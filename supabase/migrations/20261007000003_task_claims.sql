-- ORG-04: a task is owned by at most one non-terminal run. Ownership is the run's life (queued or running with a live
-- lease), so cancel, finish, failure and lease loss release it without a second record to drift. A partial unique index
-- is the guarantee; the task row lock orders concurrent acquisitions and issues the generation.
-- Retry deduplication (requester + idempotency key) stays a separate check that runs first.
begin;

alter table public.tasks add column if not exists generation bigint not null default 0;
alter table public.work_items add column if not exists task_id uuid;
alter table public.work_items add column if not exists task_generation bigint;
do $$ begin
    alter table public.work_items add constraint work_items_task_fk foreign key (workspace_id, task_id) references public.tasks (workspace_id, task_id);
exception when duplicate_object then null; end $$;
create unique index if not exists work_task_active on public.work_items (workspace_id, task_id)
    where task_id is not null and state in ('queued', 'running');

-- Who owns the task now. Visible to workspace members, who already see tasks (ORG-01); null when nothing holds it.
-- Members may not see the owning run itself (cloud_work_read), so the run's work_id is not part of the answer. A started
-- run whose lease lapsed has lost the task even before a sweep marks it failed; an unstarted one returns to the queue
-- and keeps it, so it is reported as queued.
create or replace function public.cloud_task_owner(p_workspace_id uuid, p_task_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare w public.work_items; live boolean;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    select * into w from public.work_items where workspace_id = p_workspace_id and task_id = p_task_id and state in ('queued', 'running');
    if not found then return null; end if;
    live := w.state = 'queued' or w.leased_until > statement_timestamp();
    if not live and w.started_at is not null then return null; end if;
    return jsonb_build_object('owner_id', w.requester_id, 'state', case when live then w.state else 'queued' end,
        'generation', w.task_generation, 'lease_expires_at', case when live then w.leased_until end, 'owner_name', (select m.display_name from gah_private.memberships m
            where m.workspace_id = w.workspace_id and m.user_id = w.requester_id));
end;
$$;
revoke all on function public.cloud_task_owner(uuid, uuid) from public, anon;
grant execute on function public.cloud_task_owner(uuid, uuid) to authenticated;

-- ponytail: a profile with no repository alias is not coordinated; ORG-05 makes dispatch refuse such organization work.
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
        update public.tasks set generation = generation + 1 where workspace_id = t.workspace_id and task_id = t.task_id returning generation into w.task_generation;
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

commit;
