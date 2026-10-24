-- ORG-10: who sees what, who changed what, and how long records live.
-- Visibility (decision pending Q10; default keeps today's behavior): 'workspace' shows every repository, task and lock
-- to every member. 'granted' shows only repositories the member was granted, and workspace owners always see all.
-- A repository grant is separate from node (compute/inference) grants and from entitlement; it limits what an agent
-- may run for its requester, rechecked at each enqueue, claim, renew and finish through job_allowed.
-- Member-level usage (ORG-08) is for workspace owners and the member themself.
begin;

alter table public.workspaces add column if not exists visibility text not null default 'workspace' check (visibility in ('workspace', 'granted'));
alter table public.workspaces add column if not exists event_retention_days integer not null default 90 check (event_retention_days between 7 and 3650);
alter table public.workspaces add column if not exists audit_retention_days integer not null default 365 check (audit_retention_days between 30 and 3650);

create table if not exists gah_private.repository_grants (
    workspace_id uuid not null, repo_id uuid not null, user_id uuid not null,
    primary key (workspace_id, repo_id, user_id),
    foreign key (workspace_id, repo_id) references public.repositories on delete cascade,
    foreign key (workspace_id, user_id) references gah_private.memberships on delete cascade
);
alter table gah_private.repository_grants enable row level security;
revoke all on gah_private.repository_grants from public, anon, authenticated;
grant all on gah_private.repository_grants to service_role;

create or replace function gah_private.repo_visible(p_workspace uuid, p_repo uuid, p_user uuid)
returns boolean language sql stable security definer set search_path = '' as $$
    select gah_private.member_active(p_workspace, p_user) and (
        (select visibility from public.workspaces where id = p_workspace) = 'workspace'
        or exists (select 1 from gah_private.memberships m where m.workspace_id = p_workspace and m.user_id = p_user and m.role = 'owner')
        or exists (select 1 from gah_private.repository_grants g where g.workspace_id = p_workspace and g.repo_id = p_repo and g.user_id = p_user));
$$;
-- The three-argument form answers for any user, so it would reveal other people's membership; only definer code calls it.
revoke all on function gah_private.repo_visible(uuid, uuid, uuid) from public, anon, authenticated;
-- Callers and policies ask only about themselves.
create or replace function gah_private.repo_visible(p_workspace uuid, p_repo uuid)
returns boolean language sql stable security definer set search_path = '' as $$
    select gah_private.repo_visible(p_workspace, p_repo, auth.uid());
$$;
revoke all on function gah_private.repo_visible(uuid, uuid) from public, anon;
grant execute on function gah_private.repo_visible(uuid, uuid) to authenticated;

drop policy if exists repositories_read on public.repositories;
create policy repositories_read on public.repositories for select to authenticated using (gah_private.repo_visible(workspace_id, repo_id));
drop policy if exists tasks_read on public.tasks;
create policy tasks_read on public.tasks for select to authenticated using (gah_private.repo_visible(workspace_id, repo_id));
drop policy if exists file_locks_read on public.file_locks;
create policy file_locks_read on public.file_locks for select to authenticated using (gah_private.repo_visible(workspace_id, repo_id));
-- A node owner or manager sees the runs on their node only for repositories they may see; requesters always see their
-- own runs. Events follow runs (cloud_events_read), so they are filtered the same way.
drop policy if exists cloud_work_read on public.work_items;
create policy cloud_work_read on public.work_items for select to authenticated using (gah_private.can_access(workspace_id)
    and (requester_id = auth.uid() or (gah_private.node_permission(workspace_id, node_id) in ('owner', 'manage')
        and (task_id is null or exists (select 1 from public.tasks t where t.workspace_id = work_items.workspace_id and t.task_id = work_items.task_id
            and gah_private.repo_visible(t.workspace_id, t.repo_id))))));

-- The owner lookup respects repository visibility, so a hidden repository's task stays hidden. As in ORG-04 it omits the
-- run's work_id and does not report a started run whose lease lapsed; a lapsed unstarted run is reported as queued.
create or replace function public.cloud_task_owner(p_workspace_id uuid, p_task_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare w public.work_items; live boolean;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    if not exists (select 1 from public.tasks t where t.workspace_id = p_workspace_id and t.task_id = p_task_id
        and gah_private.repo_visible(t.workspace_id, t.repo_id)) then return null; end if;
    select * into w from public.work_items where workspace_id = p_workspace_id and task_id = p_task_id and state in ('queued', 'running');
    if not found then return null; end if;
    live := w.state = 'queued' or w.leased_until > statement_timestamp();
    if not live and w.started_at is not null then return null; end if;
    return jsonb_build_object('owner_id', w.requester_id, 'state', case when live then w.state else 'queued' end,
        'generation', w.task_generation, 'lease_expires_at', case when live then w.leased_until end, 'owner_name', (select m.display_name from gah_private.memberships m
            where m.workspace_id = w.workspace_id and m.user_id = w.requester_id));
end;
$$;

-- A run is allowed only while its requester may see the repository its profile maps to. Unmapped profiles are unaffected.
create or replace function gah_private.job_allowed(w public.work_items)
returns boolean language sql stable security definer set search_path = '' as $$
    select exists (select 1 from public.nodes n where n.workspace_id = w.workspace_id and n.node_id = w.node_id
        and n.revoked_at is null and w.profile = any(n.profiles) and w.memory_mb <= n.memory_budget_mb)
    and gah_private.resource_permission(w.workspace_id, w.node_id, 'compute', w.requester_id) is not null
    and (w.device_id is null or exists (select 1 from gah_private.devices d where d.id = w.device_id and d.revoked_at is null))
    and case when w.inference_node_id is null then
        gah_private.resource_permission(w.workspace_id, w.node_id, 'inference', w.requester_id) is not null
    else exists (select 1 from public.nodes i where i.workspace_id = w.workspace_id and i.node_id = w.inference_node_id
        and i.revoked_at is null and w.inference_model = any(i.inference_models)
        and gah_private.resource_permission(w.workspace_id, w.inference_node_id, 'inference', w.requester_id) is not null) end
    and coalesce((select gah_private.repo_visible(a.workspace_id, a.repo_id, w.requester_id) from public.repository_aliases a
        where a.workspace_id = w.workspace_id and a.node_id = w.node_id and a.profile = w.profile), true);
$$;
revoke all on function gah_private.job_allowed(public.work_items) from public, anon, authenticated;

create or replace function public.configure_cloud_governance(p_workspace_id uuid, p_visibility text default null,
    p_event_retention_days integer default null, p_audit_retention_days integer default null)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    update public.workspaces set visibility = coalesce(p_visibility, visibility),
        event_retention_days = coalesce(p_event_retention_days, event_retention_days),
        audit_retention_days = coalesce(p_audit_retention_days, audit_retention_days) where id = p_workspace_id;
end;
$$;

create or replace function public.set_cloud_repository_grant(p_workspace_id uuid, p_repo_id uuid, p_user_id uuid, p_action text)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    if p_action = 'grant' then
        if not exists (select 1 from public.repositories where workspace_id = p_workspace_id and repo_id = p_repo_id)
            or not gah_private.member_active(p_workspace_id, p_user_id) then raise exception 'invalid_grant' using errcode = '22023'; end if;
        insert into gah_private.repository_grants values (p_workspace_id, p_repo_id, p_user_id) on conflict do nothing;
    elsif p_action = 'revoke' then
        delete from gah_private.repository_grants where workspace_id = p_workspace_id and repo_id = p_repo_id and user_id = p_user_id;
    else raise exception 'invalid_grant_action' using errcode = '22023'; end if;
end;
$$;

-- Audit: actor, target, and time for every administrative change. Triggers capture all paths, including future ones.
-- Only allowlisted identifying fields are copied, so credentials and token hashes never reach the log. A null actor is
-- an operator or service action. Denied attempts raise errors and are not logged.
create table if not exists gah_private.audit_log (
    id bigint generated always as identity primary key,
    workspace_id uuid not null,
    occurred_at timestamptz not null default now(),
    actor_id uuid,
    action text not null,
    target jsonb not null default '{}'
);
create index if not exists audit_log_workspace on gah_private.audit_log (workspace_id, id);
alter table gah_private.audit_log enable row level security;
revoke all on gah_private.audit_log from public, anon, authenticated;
grant all on gah_private.audit_log to service_role;

create or replace function gah_private.audit_change() returns trigger
language plpgsql security definer set search_path = '' as $$
declare r jsonb := to_jsonb(coalesce(new, old)); picked jsonb;
begin
    select coalesce(jsonb_object_agg(case when k = 'id' then 'device_id' else k end, v), '{}') into picked from jsonb_each(r) e(k, v)
        where k = any (array['user_id', 'role', 'node_id', 'resource', 'permission', 'sharing_scope', 'inference_scope', 'repo_id',
            'coordinated', 'visibility', 'event_retention_days', 'audit_retention_days', 'revoked_at'])
        or (k = 'id' and tg_table_name = 'devices');
    insert into gah_private.audit_log (workspace_id, actor_id, action, target)
    values (coalesce(r->>'workspace_id', r->>'id')::uuid, auth.uid(), tg_table_name || '.' || lower(tg_op), picked);
    return null;
end;
$$;
revoke all on function gah_private.audit_change() from public, anon, authenticated;
drop trigger if exists audit_memberships on gah_private.memberships;
create trigger audit_memberships after insert or delete or update of role on gah_private.memberships for each row execute function gah_private.audit_change();
drop trigger if exists audit_node_grants on gah_private.node_grants;
create trigger audit_node_grants after insert or delete or update on gah_private.node_grants for each row execute function gah_private.audit_change();
drop trigger if exists audit_dashboard_grants on gah_private.dashboard_grants;
create trigger audit_dashboard_grants after insert or delete on gah_private.dashboard_grants for each row execute function gah_private.audit_change();
drop trigger if exists audit_repository_grants on gah_private.repository_grants;
create trigger audit_repository_grants after insert or delete on gah_private.repository_grants for each row execute function gah_private.audit_change();
drop trigger if exists audit_devices on gah_private.devices;
create trigger audit_devices after insert or update of revoked_at on gah_private.devices for each row execute function gah_private.audit_change();
drop trigger if exists audit_nodes on public.nodes;
create trigger audit_nodes after update of sharing_scope, inference_scope, revoked_at on public.nodes for each row execute function gah_private.audit_change();
drop trigger if exists audit_workspaces on public.workspaces;
create trigger audit_workspaces after update of coordinated, visibility, event_retention_days, audit_retention_days on public.workspaces
    for each row execute function gah_private.audit_change();

-- Owner-only read and export of the audit log, paged by id.
create or replace function public.cloud_audit_log(p_workspace_id uuid, p_after bigint default 0, p_limit integer default 200)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    return coalesce((select jsonb_agg(to_jsonb(x) order by x.id) from (
        select id, occurred_at, actor_id, action, target from gah_private.audit_log
        where workspace_id = p_workspace_id and id > greatest(p_after, 0) order by id limit least(greatest(p_limit, 1), 500)) x), '[]');
end;
$$;

-- Retention, run by an operator job (service role). Cursor readers see the pruned range as a gap (ORG-03).
create or replace function public.prune_cloud_retention()
returns jsonb language plpgsql security definer set search_path = '' as $$
declare e bigint; a bigint;
begin
    delete from public.events v using public.workspaces w where v.workspace_id = w.id and v.occurred_at < statement_timestamp() - make_interval(days => w.event_retention_days);
    get diagnostics e = row_count;
    delete from gah_private.audit_log l using public.workspaces w where l.workspace_id = w.id and l.occurred_at < statement_timestamp() - make_interval(days => w.audit_retention_days);
    get diagnostics a = row_count;
    return jsonb_build_object('events', e, 'audit', a);
end;
$$;

alter table public.file_locks alter column owner_id drop not null;

-- Erase the attribution of someone who has left. Active members cannot be erased. Run records stay for totals but name no one.
-- Their events also lose the device that ran them, their locks the holder, and a pending handoff to them is dropped.
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
    update public.tasks set handoff_kind = null, handoff_to = null, handoff_expires_at = null
        where workspace_id = p_workspace_id and handoff_to = p_user_id;
    return jsonb_build_object('events', e, 'audit', a, 'runs', r);
end;
$$;

revoke all on function public.configure_cloud_governance(uuid, text, integer, integer), public.set_cloud_repository_grant(uuid, uuid, uuid, text),
    public.cloud_audit_log(uuid, bigint, integer), public.erase_departed_member(uuid, uuid) from public, anon;
grant execute on function public.configure_cloud_governance(uuid, text, integer, integer), public.set_cloud_repository_grant(uuid, uuid, uuid, text),
    public.cloud_audit_log(uuid, bigint, integer), public.erase_departed_member(uuid, uuid) to authenticated;
revoke all on function public.prune_cloud_retention() from public, anon, authenticated;
grant execute on function public.prune_cloud_retention() to service_role;

commit;
