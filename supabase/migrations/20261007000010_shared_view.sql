-- ORG-07 (server side): what coworkers see about shared work, and in-product handoff notifications.
-- cloud_shared_work lists each task's latest run with who initiated it, where it executes, and three separate statuses:
--   owner_status (is the initiating member still active: membership and entitlement), device_status (worker heartbeat),
--   and claim_status (queued, live lease, expired lease, or ended). The last observed state is the sender's last report
--   and is never presented as confirmed; confirmed state is the run state. It is read from the run's current attempt (agent
--   session) only, as in ORG-03's cloud_run_status, so a requeued run never shows an earlier attempt's report.
-- Everything is limited to repositories the caller may see (ORG-10). Human coworker workflows need no agent or device:
--   requesting, answering, and cancelling use the member's own session through the ORG-05 functions.
-- Notifications are rows created from handoff events for the people involved, never the actor. An unacknowledged
--   notification of one kind for one task is refreshed rather than duplicated. Delivery to chat or email is a separate scope.
begin;

create table if not exists gah_private.notifications (
    id bigint generated always as identity primary key,
    workspace_id uuid not null,
    user_id uuid not null,
    kind text not null,
    task_id uuid,
    event_id text,
    payload jsonb not null default '{}',
    created_at timestamptz not null default now(),
    acked_at timestamptz
);
create unique index if not exists notifications_unacked on gah_private.notifications (user_id, kind, task_id) where acked_at is null;
alter table gah_private.notifications enable row level security;
revoke all on gah_private.notifications from public, anon, authenticated;
grant all on gah_private.notifications to service_role;

create or replace function gah_private.notify_from_event() returns trigger
language plpgsql security definer set search_path = '' as $$
declare w public.work_items; task uuid; recipients uuid[];
begin
    select * into w from public.work_items where workspace_id = new.workspace_id and work_id = new.work_id;
    task := coalesce((new.payload->>'task_id')::uuid, w.task_id);
    recipients := case new.kind
        when 'task.handoff_offered' then array[(new.payload->>'to')::uuid]
        when 'task.takeover_requested' then array[w.requester_id]
        when 'task.handoff_accepted' then array[(new.payload->>'to')::uuid, w.requester_id]
        when 'task.handoff_declined' then array[(new.payload->>'to')::uuid, w.requester_id]
        when 'task.takeover' then array[(new.payload->>'to')::uuid, (new.payload->>'from')::uuid]
        when 'file.resolved' then array[w.requester_id]
    end;
    insert into gah_private.notifications (workspace_id, user_id, kind, task_id, event_id, payload)
    select new.workspace_id, u, new.kind, task, new.event_id, new.payload from unnest(recipients) u
    where u is not null and u is distinct from new.actor_id and gah_private.member_active(new.workspace_id, u)
    on conflict (user_id, kind, task_id) where acked_at is null do update set event_id = excluded.event_id, payload = excluded.payload, created_at = statement_timestamp();
    return null;
end;
$$;
revoke all on function gah_private.notify_from_event() from public, anon, authenticated;
drop trigger if exists notify_from_event on public.events;
create trigger notify_from_event after insert on public.events for each row
    when (new.kind in ('task.handoff_offered', 'task.takeover_requested', 'task.handoff_accepted', 'task.handoff_declined', 'task.takeover', 'file.resolved'))
    execute function gah_private.notify_from_event();

create or replace function public.cloud_notifications(p_workspace_id uuid, p_include_acknowledged boolean default false)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    return coalesce((select jsonb_agg(to_jsonb(x) order by x.id desc) from (
        select id, kind, task_id, payload, created_at, acked_at from gah_private.notifications
        where workspace_id = p_workspace_id and user_id = auth.uid() and (p_include_acknowledged or acked_at is null) order by id desc limit 100) x), '[]');
end;
$$;

create or replace function public.ack_cloud_notification(p_id bigint)
returns boolean language plpgsql security definer set search_path = '' as $$
begin
    update gah_private.notifications set acked_at = statement_timestamp() where id = p_id and user_id = auth.uid() and acked_at is null;
    return found;
end;
$$;

create or replace function public.cloud_shared_work(p_workspace_id uuid, p_repo_id uuid default null, p_since interval default interval '7 days', p_limit integer default 100)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    return coalesce((select jsonb_agg(row order by sort_at desc) from (
        select jsonb_build_object(
            'task_id', t.task_id, 'repository', r.path, 'source', r.source, 'kind', t.kind, 'ref', t.ref, 'generation', t.generation,
            'url', case when r.source = 'github.com' then 'https://github.com/' || r.path || case t.kind when 'review' then '/pull/' else '/issues/' end || t.ref
                        when r.source = 'gitlab.com' then 'https://gitlab.com/' || r.path || case t.kind when 'review' then '/-/merge_requests/' else '/-/issues/' end || t.ref end,
            'run', jsonb_build_object('work_id', w.work_id, 'state', w.state, 'initiated_by', m.display_name, 'initiated_by_id', w.requester_id,
                'owner_status', case when w.requester_id is null then 'former' when gah_private.member_active(w.workspace_id, w.requester_id) then 'active' else 'inactive' end,
                'node_id', w.node_id, 'worker_kind', n.worker_kind,
                'device_status', case when n.node_id is null then null when n.revoked_at is not null then 'revoked' when n.worker_heartbeat_at is null then 'unknown'
                    when n.worker_heartbeat_at > statement_timestamp() - interval '90 seconds' then 'online' else 'offline' end,
                'claim_status', case when w.state = 'queued' then 'queued' when w.state = 'running' and w.leased_until > statement_timestamp() then 'live'
                    when w.state = 'running' then 'expired' else 'ended' end,
                'agent_session_id', w.agent_session_id, 'started_at', w.started_at, 'stop_requested', w.stop_requested_at is not null,
                'last_observed_state', o.payload->>'state', 'last_observed_at', o.occurred_at),
            'locks', jsonb_build_object('held', (select count(*) from public.file_locks l where l.workspace_id = w.workspace_id and l.work_id = w.work_id and l.resolved_at is null),
                'orphaned', (select count(*) from public.file_locks l where l.workspace_id = w.workspace_id and l.work_id = w.work_id and l.resolved_at is null
                    and (w.state not in ('queued', 'running') or (w.state = 'running' and w.leased_until <= statement_timestamp())))),
            'handoff', case when t.handoff_kind is not null and t.handoff_expires_at > statement_timestamp()
                then jsonb_build_object('kind', t.handoff_kind, 'to', t.handoff_to, 'expires_at', t.handoff_expires_at) end) as row,
            w.updated_at as sort_at
        from public.tasks t join public.repositories r on r.workspace_id = t.workspace_id and r.repo_id = t.repo_id
        join lateral (select * from public.work_items x where x.workspace_id = t.workspace_id and x.task_id = t.task_id order by x.task_generation desc nulls last limit 1) w on true
        left join gah_private.memberships m on m.workspace_id = w.workspace_id and m.user_id = w.requester_id
        left join public.nodes n on n.workspace_id = w.workspace_id and n.node_id = w.node_id
        left join lateral (select e.payload, e.occurred_at from public.events e where e.workspace_id = w.workspace_id and e.work_id = w.work_id
            and e.agent_session_id = w.agent_session_id and e.kind = 'run.state' order by e.run_seq desc limit 1) o on true
        where t.workspace_id = p_workspace_id and (p_repo_id is null or t.repo_id = p_repo_id) and gah_private.repo_visible(t.workspace_id, t.repo_id, auth.uid())
            and (w.state in ('queued', 'running') or w.updated_at > statement_timestamp() - p_since)
        order by w.updated_at desc limit least(greatest(p_limit, 1), 200)) s), '[]');
end;
$$;

-- Ownership and takeover history for one task: who asked, answered, took over, and when.
create or replace function public.cloud_task_history(p_workspace_id uuid, p_task_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    if not exists (select 1 from public.tasks t where t.workspace_id = p_workspace_id and t.task_id = p_task_id and gah_private.repo_visible(t.workspace_id, t.repo_id, auth.uid())) then
        return '[]'; end if;
    return coalesce((select jsonb_agg(to_jsonb(x) order by x.seq) from (
        select seq, kind, occurred_at, actor_id, work_id, payload from public.events
        where workspace_id = p_workspace_id and (payload->>'task_id' = p_task_id::text or work_id in (select work_id from public.work_items where workspace_id = p_workspace_id and task_id = p_task_id))
            and kind in ('task.claimed', 'task.handoff_offered', 'task.takeover_requested', 'task.handoff_accepted', 'task.handoff_declined', 'task.handoff_completed', 'task.takeover',
                'cloud.handoff_stopped', 'cloud.lease_expired', 'file.resolved', 'run.retried') order by seq limit 500) x), '[]');
end;
$$;

-- Notifications are derived from events, so they share the event retention window (ORG-10).
create or replace function public.prune_cloud_retention()
returns jsonb language plpgsql security definer set search_path = '' as $$
declare e bigint; a bigint; n bigint;
begin
    delete from public.events v using public.workspaces w where v.workspace_id = w.id and v.occurred_at < statement_timestamp() - make_interval(days => w.event_retention_days);
    get diagnostics e = row_count;
    delete from gah_private.audit_log l using public.workspaces w where l.workspace_id = w.id and l.occurred_at < statement_timestamp() - make_interval(days => w.audit_retention_days);
    get diagnostics a = row_count;
    delete from gah_private.notifications x using public.workspaces w where x.workspace_id = w.id and x.created_at < statement_timestamp() - make_interval(days => w.event_retention_days);
    get diagnostics n = row_count;
    return jsonb_build_object('events', e, 'audit', a, 'notifications', n);
end;
$$;

-- Erasure (ORG-10) also removes a departed member's notifications and their name from others' notifications.
create or replace function public.erase_departed_member(p_workspace_id uuid, p_user_id uuid)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare e bigint; a bigint; r bigint;
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    if exists (select 1 from gah_private.memberships where workspace_id = p_workspace_id and user_id = p_user_id) then raise exception 'member_still_active' using errcode = '22023'; end if;
    update public.events set actor_id = case when actor_id = p_user_id then null else actor_id end, device_id = case when actor_id = p_user_id then null else device_id end, payload = payload - 'to' - 'from'
        where workspace_id = p_workspace_id and (actor_id = p_user_id or payload->>'to' = p_user_id::text or payload->>'from' = p_user_id::text);
    get diagnostics e = row_count;
    update gah_private.audit_log set actor_id = case when actor_id = p_user_id then null else actor_id end,
        target = case when target->>'user_id' = p_user_id::text then target - 'user_id' else target end
        where workspace_id = p_workspace_id and (actor_id = p_user_id or target->>'user_id' = p_user_id::text);
    get diagnostics a = row_count;
    update public.work_items set requester_id = null where workspace_id = p_workspace_id and requester_id = p_user_id and state not in ('queued', 'running');
    get diagnostics r = row_count;
    update public.file_locks set owner_id = null where workspace_id = p_workspace_id and owner_id = p_user_id;
    update gah_private.admissions set payer_id = null where workspace_id = p_workspace_id and payer_id = p_user_id;
    delete from gah_private.notifications where workspace_id = p_workspace_id and user_id = p_user_id;
    update gah_private.notifications set payload = payload - 'to' - 'from'
        where workspace_id = p_workspace_id and (payload->>'to' = p_user_id::text or payload->>'from' = p_user_id::text);
    update public.tasks set handoff_kind = null, handoff_to = null, handoff_expires_at = null
        where workspace_id = p_workspace_id and handoff_to = p_user_id;
    return jsonb_build_object('events', e, 'audit', a, 'runs', r);
end;
$$;

revoke all on function public.cloud_notifications(uuid, boolean), public.ack_cloud_notification(bigint), public.cloud_shared_work(uuid, uuid, interval, integer),
    public.cloud_task_history(uuid, uuid) from public, anon;
grant execute on function public.cloud_notifications(uuid, boolean), public.ack_cloud_notification(bigint), public.cloud_shared_work(uuid, uuid, interval, integer),
    public.cloud_task_history(uuid, uuid) to authenticated;

commit;
