-- ORG-05: ownership is checked while a run executes, and moves between coworkers only through an audited handoff.
-- Handoff completes when the prior run is terminal, which is also when its publication check starts failing, so no
-- participating path can publish after a successor exists. A process outside these paths is not fenced (see ORG-06).
-- Stop requests reach a running process at its next renewal, at most one 30 second lease later.
begin;

-- Opt-in per workspace so existing pilots keep working until their profiles are mapped.
alter table public.workspaces add column if not exists coordinated boolean not null default false;
alter table public.tasks add column if not exists handoff_kind text check (handoff_kind in ('offer', 'request', 'approved'));
alter table public.tasks add column if not exists handoff_to uuid;
alter table public.tasks add column if not exists handoff_expires_at timestamptz;
alter table public.work_items add column if not exists stop_requested_at timestamptz;

create or replace function public.configure_cloud_coordination(p_workspace_id uuid, p_enabled boolean)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    update public.workspaces set coordinated = coalesce(p_enabled, false) where id = p_workspace_id;
end;
$$;

-- Handoff history is the event log: who asked, who answered, who was stopped.
create or replace function gah_private.task_event(p_task public.tasks, p_work text, p_kind text, p_payload jsonb)
returns void language sql security definer set search_path = '' as $$
    insert into public.events (workspace_id, event_id, work_id, kind, actor_id, payload)
    values (p_task.workspace_id, gen_random_uuid()::text, p_work, p_kind, auth.uid(),
        p_payload || jsonb_build_object('task_id', p_task.task_id));
$$;
revoke all on function gah_private.task_event(public.tasks, text, text, jsonb) from public, anon, authenticated;

-- A queued run is cancelled outright. A running run is told to stop and ends at its next renewal or lease lapse.
-- w may be stale: a claim that lands after the caller read it is told to stop, never cancelled with a live lease.
create or replace function gah_private.stop_run(w public.work_items)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if w.state = 'queued' then
        update public.work_items set state = 'cancelled', updated_at = statement_timestamp()
            where workspace_id = w.workspace_id and work_id = w.work_id and state = 'queued';
        if found then perform gah_private.audit(w, 'cloud.handoff_stopped'); return; end if;
    end if;
    update public.work_items set stop_requested_at = statement_timestamp()
        where workspace_id = w.workspace_id and work_id = w.work_id and state = 'running';
end;
$$;
revoke all on function gah_private.stop_run(public.work_items) from public, anon, authenticated;

-- Common start: lock the task, find its live run, drop an expired or stale handoff.
create or replace function gah_private.handoff_context(p_workspace_id uuid, p_task_id uuid, out t public.tasks, out w public.work_items)
language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    select * into t from public.tasks where workspace_id = p_workspace_id and task_id = p_task_id for update;
    if not found then raise exception 'task_not_found' using errcode = '22023'; end if;
    select * into w from public.work_items where workspace_id = p_workspace_id and task_id = p_task_id and state in ('queued', 'running');
    if t.handoff_kind is not null and (t.handoff_expires_at <= statement_timestamp() or (t.handoff_kind <> 'approved' and w.work_id is null)) then
        update public.tasks set handoff_kind = null, handoff_to = null, handoff_expires_at = null where workspace_id = t.workspace_id and task_id = t.task_id
            returning * into t;
    end if;
end;
$$;
revoke all on function gah_private.handoff_context(uuid, uuid) from public, anon, authenticated;

-- The owner offers the task to a coworker, who must accept.
create or replace function public.offer_cloud_handoff(p_workspace_id uuid, p_task_id uuid, p_to uuid)
returns void language plpgsql security definer set search_path = '' as $$
declare h record; t public.tasks; w public.work_items;
begin
    select * into h from gah_private.handoff_context(p_workspace_id, p_task_id); t := h.t; w := h.w;
    if w.work_id is null or w.requester_id is distinct from auth.uid() then raise exception 'task_owner_required' using errcode = '42501'; end if;
    if p_to = auth.uid() or not gah_private.member_active(p_workspace_id, p_to) then raise exception 'invalid_handoff_target' using errcode = '22023'; end if;
    if t.handoff_kind is not null then raise exception 'handoff_in_progress' using errcode = '55006'; end if;
    update public.tasks set handoff_kind = 'offer', handoff_to = p_to, handoff_expires_at = statement_timestamp() + interval '1 hour'
        where workspace_id = p_workspace_id and task_id = p_task_id;
    perform gah_private.task_event(t, w.work_id, 'task.handoff_offered', jsonb_build_object('to', p_to));
end;
$$;

-- A coworker asks the owner for the task. The owner decides.
create or replace function public.request_cloud_takeover(p_workspace_id uuid, p_task_id uuid)
returns void language plpgsql security definer set search_path = '' as $$
declare h record; t public.tasks; w public.work_items;
begin
    select * into h from gah_private.handoff_context(p_workspace_id, p_task_id); t := h.t; w := h.w;
    if w.work_id is null or w.requester_id = auth.uid() then raise exception 'task_not_owned_by_another' using errcode = '22023'; end if;
    if t.handoff_kind is not null then raise exception 'handoff_in_progress' using errcode = '55006'; end if;
    update public.tasks set handoff_kind = 'request', handoff_to = auth.uid(), handoff_expires_at = statement_timestamp() + interval '1 hour'
        where workspace_id = p_workspace_id and task_id = p_task_id;
    perform gah_private.task_event(t, w.work_id, 'task.takeover_requested', jsonb_build_object('to', auth.uid()));
end;
$$;

-- The offered coworker (for an offer) or the current owner (for a request) answers. Accepting stops the current run
-- and reserves the task for the recipient for ten minutes.
create or replace function public.respond_cloud_handoff(p_workspace_id uuid, p_task_id uuid, p_accept boolean)
returns void language plpgsql security definer set search_path = '' as $$
declare h record; t public.tasks; w public.work_items;
begin
    select * into h from gah_private.handoff_context(p_workspace_id, p_task_id); t := h.t; w := h.w;
    if coalesce(t.handoff_kind, '') not in ('offer', 'request') then raise exception 'no_pending_handoff' using errcode = '22023'; end if;
    if auth.uid() is distinct from (case t.handoff_kind when 'offer' then t.handoff_to else w.requester_id end) then
        raise exception 'handoff_response_denied' using errcode = '42501'; end if;
    if not coalesce(p_accept, false) then
        update public.tasks set handoff_kind = null, handoff_to = null, handoff_expires_at = null where workspace_id = p_workspace_id and task_id = p_task_id;
        perform gah_private.task_event(t, w.work_id, 'task.handoff_declined', jsonb_build_object('to', t.handoff_to));
        return;
    end if;
    perform gah_private.stop_run(w);
    update public.tasks set handoff_kind = 'approved', handoff_expires_at = statement_timestamp() + interval '10 minutes'
        where workspace_id = p_workspace_id and task_id = p_task_id;
    perform gah_private.task_event(t, w.work_id, 'task.handoff_accepted', jsonb_build_object('to', t.handoff_to, 'from', w.requester_id));
end;
$$;

-- A workspace owner or the owner/manager of the run's node takes the task without the holder's consent.
create or replace function public.force_cloud_takeover(p_workspace_id uuid, p_task_id uuid, p_to uuid default null)
returns void language plpgsql security definer set search_path = '' as $$
declare h record; t public.tasks; w public.work_items; target uuid := coalesce(p_to, auth.uid());
begin
    select * into h from gah_private.handoff_context(p_workspace_id, p_task_id); t := h.t; w := h.w;
    if w.work_id is null then raise exception 'task_not_claimed' using errcode = '22023'; end if;
    if not (gah_private.workspace_owner(p_workspace_id) or gah_private.node_permission(p_workspace_id, w.node_id) in ('owner', 'manage')) then
        raise exception 'takeover_denied' using errcode = '42501'; end if;
    if not gah_private.member_active(p_workspace_id, target) then raise exception 'invalid_handoff_target' using errcode = '22023'; end if;
    perform gah_private.stop_run(w);
    update public.tasks set handoff_kind = 'approved', handoff_to = target, handoff_expires_at = statement_timestamp() + interval '10 minutes'
        where workspace_id = p_workspace_id and task_id = p_task_id;
    perform gah_private.task_event(t, w.work_id, 'task.takeover', jsonb_build_object('to', target, 'from', w.requester_id));
end;
$$;

-- Call before any GAH-controlled publication or other shared mutation. It does not extend the lease.
create or replace function public.check_cloud_ownership(p_device_token text, p_work_id text, p_claim_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; why text;
begin
    select * into w from public.work_items where workspace_id = d.workspace_id and work_id = p_work_id and node_id = d.node_id
        and device_id = d.id and claim_id = p_claim_id;
    why := case when not found or w.state <> 'running' then 'not_running'
        when w.leased_until <= statement_timestamp() then 'lease_expired'
        when w.stop_requested_at is not null then 'stop_requested'
        when not gah_private.job_allowed(w) then 'access_revoked'
        when w.task_id is not null and w.task_generation is distinct from (select generation from public.tasks where workspace_id = w.workspace_id and task_id = w.task_id) then 'superseded'
    end;
    return jsonb_build_object('owned', why is null, 'reason', why, 'generation', w.task_generation, 'lease_expires_at', w.leased_until);
end;
$$;

create or replace function public.renew_cloud_work(p_device_token text, p_work_id text, p_claim_id uuid, p_start boolean default false)
returns boolean language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items;
begin
    select * into w from public.work_items where workspace_id = d.workspace_id and work_id = p_work_id
        and node_id = d.node_id and device_id = d.id and claim_id = p_claim_id and state = 'running'
        and leased_until > statement_timestamp() for update;
    if not found then return false; end if;
    if w.stop_requested_at is not null or not gah_private.job_allowed(w) then
        update public.work_items set state = 'cancelled', leased_until = null, updated_at = statement_timestamp()
        where workspace_id = w.workspace_id and work_id = w.work_id;
        perform gah_private.audit(w, case when w.stop_requested_at is not null then 'cloud.handoff_stopped' else 'cloud.access_revoked' end);
        return false;
    end if;
    if p_start and w.started_at is not null then return false; end if;
    update public.work_items set started_at = case when p_start then statement_timestamp() else started_at end,
        leased_until = statement_timestamp() + interval '30 seconds', updated_at = statement_timestamp()
    where workspace_id = w.workspace_id and work_id = w.work_id;
    if p_start then perform gah_private.audit(w, 'cloud.started'); end if;
    return true;
end;
$$;

-- A stopped run never reports success: its work may already be unowned.
create or replace function public.finish_cloud_work(p_device_token text, p_work_id text, p_claim_id uuid, p_exit_code integer)
returns boolean language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items;
begin
    update public.work_items x set state = case when x.stop_requested_at is not null or not gah_private.job_allowed(x) then 'cancelled'
            when p_exit_code = 0 then 'completed' else 'failed' end,
        exit_code = p_exit_code, leased_until = null, updated_at = statement_timestamp()
    where x.workspace_id = d.workspace_id and x.node_id = d.node_id and x.work_id = p_work_id and x.device_id = d.id
        and x.claim_id = p_claim_id and x.state = 'running' and x.leased_until > statement_timestamp()
        and (x.started_at is not null or p_exit_code <> 0) returning * into w;
    if not found then return false; end if;
    perform gah_private.audit(w, 'cloud.' || w.state); return true;
end;
$$;

-- Acquisition honors a handoff reservation and, in a coordinated workspace, refuses profiles with no repository.
create or replace function public.enqueue_cloud_work(p_workspace_id uuid, p_node_id text, p_profile text,
    p_mode text, p_target integer, p_memory_mb integer, p_idempotency_key uuid,
    p_inference_node_id text default null, p_inference_model text default null)
returns text language plpgsql security definer set search_path = '' as $$
declare w public.work_items; prior public.work_items; r uuid; t public.tasks; held public.work_items; lost public.work_items;
begin
    -- A node lock serializes admission and same-node idempotent submissions.
    perform 1 from public.nodes where workspace_id = p_workspace_id and node_id = p_node_id for update;
    w.workspace_id := p_workspace_id; w.work_id := gen_random_uuid()::text; w.node_id := p_node_id;
    w.requester_id := auth.uid(); w.profile := p_profile; w.mode := p_mode; w.target := p_target;
    w.memory_mb := p_memory_mb; w.idempotency_key := p_idempotency_key; w.state := 'queued';
    w.inference_node_id := p_inference_node_id; w.inference_model := p_inference_model;
    if p_mode is null or p_mode not in ('improve', 'review') or p_target is null or p_target < 1
        or p_memory_mb is null or p_memory_mb < 64 or p_idempotency_key is null
        or (p_inference_node_id is null) <> (p_inference_model is null) then
        raise exception 'invalid_work_request' using errcode = '22023';
    end if;
    if not gah_private.job_allowed(w) then raise exception 'work_access_denied' using errcode = '42501'; end if;
    select * into prior from public.work_items where workspace_id = p_workspace_id and requester_id = auth.uid() and idempotency_key = p_idempotency_key;
    if found then
        if (prior.node_id, prior.profile, prior.mode, prior.target, prior.memory_mb, prior.inference_node_id, prior.inference_model)
            is distinct from (w.node_id, w.profile, w.mode, w.target, w.memory_mb, w.inference_node_id, w.inference_model) then
            raise exception 'idempotency_conflict' using errcode = '22023';
        end if;
        return prior.work_id;
    end if;
    if (select count(*) from public.work_items where workspace_id = p_workspace_id and requester_id = auth.uid() and state in ('queued', 'running')) >= 100 then
        raise exception 'queue_limit' using errcode = '54000';
    end if;
    select repo_id into r from public.repository_aliases where workspace_id = p_workspace_id and node_id = p_node_id and profile = p_profile;
    if r is null and (select coordinated from public.workspaces where id = p_workspace_id) then
        raise exception 'repository_unmapped' using errcode = '22023';
    end if;
    if r is not null then
        insert into public.tasks (workspace_id, repo_id, kind, ref) values (p_workspace_id, r, case p_mode when 'improve' then 'issue' else 'review' end, p_target::text)
            on conflict (workspace_id, repo_id, kind, ref) do nothing;
        select * into t from public.tasks where workspace_id = p_workspace_id and repo_id = r
            and kind = case p_mode when 'improve' then 'issue' else 'review' end and ref = p_target::text for update;
        -- A lost lease ends the holder: a started run is terminal (never run twice), an unstarted one returns to the queue.
        for lost in update public.work_items set state = case when started_at is null then 'queued' else 'failed' end,
                claim_id = case when started_at is null then null else claim_id end,
                device_id = case when started_at is null then null else device_id end,
                agent_session_id = case when started_at is null then null else agent_session_id end,
                leased_until = null, updated_at = statement_timestamp()
            where workspace_id = t.workspace_id and task_id = t.task_id and state = 'running' and leased_until <= statement_timestamp() returning * loop
            if lost.state = 'failed' then perform gah_private.audit(lost, 'cloud.lease_expired'); end if;
        end loop;
        select * into held from public.work_items where workspace_id = t.workspace_id and task_id = t.task_id and state in ('queued', 'running');
        if found then
            raise exception 'task_claimed' using errcode = '55006', detail = public.cloud_task_owner(t.workspace_id, t.task_id)::text;
        end if;
        -- The prior run is terminal. An approved handoff reserves the task for its recipient until it expires.
        if t.handoff_kind = 'approved' and t.handoff_expires_at > statement_timestamp() then
            if t.handoff_to is distinct from auth.uid() then
                raise exception 'task_reserved' using errcode = '55006', detail = jsonb_build_object('reserved_for', t.handoff_to, 'until', t.handoff_expires_at)::text;
            end if;
            perform gah_private.task_event(t, null, 'task.handoff_completed', jsonb_build_object('to', auth.uid()));
        end if;
        update public.tasks set generation = generation + 1, handoff_kind = null, handoff_to = null, handoff_expires_at = null
            where workspace_id = t.workspace_id and task_id = t.task_id returning generation into w.task_generation;
        w.task_id := t.task_id;
    end if;
    insert into public.work_items (workspace_id, work_id, node_id, requester_id, profile, mode, target, memory_mb,
        idempotency_key, state, inference_node_id, inference_model, task_id, task_generation)
    values (w.workspace_id, w.work_id, w.node_id, w.requester_id, w.profile, w.mode, w.target, w.memory_mb,
        w.idempotency_key, w.state, w.inference_node_id, w.inference_model, w.task_id, w.task_generation);
    perform gah_private.audit(w, 'cloud.queued');
    if w.task_id is not null then
        insert into public.events (workspace_id, event_id, work_id, kind, actor_id, payload)
        values (w.workspace_id, gen_random_uuid()::text, w.work_id, 'task.claimed', w.requester_id,
            jsonb_build_object('task_id', w.task_id, 'generation', w.task_generation));
    end if;
    return w.work_id;
end;
$$;

revoke all on function public.configure_cloud_coordination(uuid, boolean), public.offer_cloud_handoff(uuid, uuid, uuid),
    public.request_cloud_takeover(uuid, uuid), public.respond_cloud_handoff(uuid, uuid, boolean),
    public.force_cloud_takeover(uuid, uuid, uuid) from public, anon;
grant execute on function public.configure_cloud_coordination(uuid, boolean), public.offer_cloud_handoff(uuid, uuid, uuid),
    public.request_cloud_takeover(uuid, uuid), public.respond_cloud_handoff(uuid, uuid, boolean),
    public.force_cloud_takeover(uuid, uuid, uuid) to authenticated;
revoke all on function public.check_cloud_ownership(text, text, uuid) from public, anon, authenticated, service_role;
grant execute on function public.check_cloud_ownership(text, text, uuid) to anon, authenticated;

commit;
