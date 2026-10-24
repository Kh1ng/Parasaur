-- ORG-12: choose a worker, explain the choice, and retry without running anything twice.
-- Eligibility mirrors job_allowed plus liveness and the requested location. At capacity is a queue reason, not a
-- refusal: the run waits for a free slot on an eligible worker. Default placement picks the least loaded online worker.
-- Explicit placement is enqueue_cloud_work with a node. A run is never moved or retried automatically; retry is an
-- explicit act of the original requester, so the payer never changes, and each attempt keeps the task and gets its own
-- work_id. A started run whose outcome is unknown needs acknowledgement and resolved file locks before it can retry.
-- Admission accounting here covers cloud-queued work only; sharing it with local dispatch needs a core contract.
begin;

alter table public.nodes add column if not exists worker_kind text not null default 'existing' check (worker_kind in ('existing', 'managed'));
alter table public.work_items add column if not exists retry_of text;

create or replace function gah_private.placement(p_workspace_id uuid, p_profile text, p_mode text, p_target integer, p_memory_mb integer,
    p_location text, p_inference_node_id text, p_inference_model text)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare n public.nodes; reasons text[]; running integer; repo uuid; rows jsonb := '[]'; busy_task boolean := false; me uuid := auth.uid();
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    if p_location is null or p_location not in ('any', 'existing', 'managed') then raise exception 'invalid_location' using errcode = '22023'; end if;
    for n in select * from public.nodes where workspace_id = p_workspace_id and gah_private.node_permission(workspace_id, node_id) is not null order by node_id loop
        reasons := '{}';
        select repo_id into repo from public.repository_aliases a where a.workspace_id = n.workspace_id and a.node_id = n.node_id and a.profile = p_profile;
        select count(*) into running from public.work_items where workspace_id = n.workspace_id and node_id = n.node_id and state = 'running';
        if n.revoked_at is not null then reasons := array_append(reasons, 'revoked'::text); end if;
        if not (p_profile = any (n.profiles)) then reasons := array_append(reasons, 'profile_unsupported'::text); end if;
        if p_memory_mb > n.memory_budget_mb then reasons := array_append(reasons, 'memory_exceeds_budget'::text); end if;
        if p_location <> 'any' and n.worker_kind <> p_location then reasons := array_append(reasons, 'location_excluded'::text); end if;
        if n.worker_heartbeat_at is null or n.worker_heartbeat_at <= statement_timestamp() - interval '90 seconds' then reasons := array_append(reasons, 'offline'::text); end if;
        if p_inference_node_id is null then
            if gah_private.resource_permission(n.workspace_id, n.node_id, 'inference', me) is null then reasons := array_append(reasons, 'inference_not_permitted'::text); end if;
        elsif not exists (select 1 from public.nodes i where i.workspace_id = n.workspace_id and i.node_id = p_inference_node_id and i.revoked_at is null
            and p_inference_model = any (i.inference_models) and gah_private.resource_permission(i.workspace_id, i.node_id, 'inference', me) is not null) then
            reasons := array_append(reasons, 'inference_not_permitted'::text);
        end if;
        if repo is null and (select coordinated from public.workspaces where id = p_workspace_id) then reasons := array_append(reasons, 'repository_unmapped'::text);
        elsif repo is not null then
            if not gah_private.repo_visible(n.workspace_id, repo, me) then reasons := array_append(reasons, 'repository_not_visible'::text); end if;
            if exists (select 1 from public.tasks t join public.work_items w on w.workspace_id = t.workspace_id and w.task_id = t.task_id
                where t.workspace_id = n.workspace_id and t.repo_id = repo and t.kind = case p_mode when 'review' then 'review' else 'issue' end
                    and t.ref = p_target::text and (w.state = 'queued' or (w.state = 'running' and w.leased_until > statement_timestamp()))) then
                reasons := array_append(reasons, 'task_claimed'::text); end if;
        end if;
        rows := rows || jsonb_build_object('node_id', n.node_id, 'worker_kind', n.worker_kind, 'eligible', cardinality(reasons) = 0, 'reasons', to_jsonb(reasons),
            'running', running, 'max_concurrent', n.max_concurrent, 'queued_reason', case when cardinality(reasons) = 0 and running >= n.max_concurrent then 'at_capacity' end);
    end loop;
    return jsonb_build_object('candidates', rows, 'chosen', (select c->>'node_id' from jsonb_array_elements(rows) c where (c->>'eligible')::boolean
        order by (c->>'running')::numeric / greatest((c->>'max_concurrent')::numeric, 1), c->>'node_id' limit 1));
end;
$$;
revoke all on function gah_private.placement(uuid, text, text, integer, integer, text, text, text) from public, anon, authenticated;

create or replace function public.explain_cloud_placement(p_workspace_id uuid, p_profile text, p_mode text, p_target integer, p_memory_mb integer,
    p_location text default 'any', p_inference_node_id text default null, p_inference_model text default null)
returns jsonb language sql stable security definer set search_path = '' as $$
    select gah_private.placement(p_workspace_id, p_profile, p_mode, p_target, p_memory_mb, p_location, p_inference_node_id, p_inference_model);
$$;

-- Default placement. Refuses with every candidate's reasons when nothing is eligible. A retry with the same key returns
-- the run the first call placed, wherever it went, instead of placing again.
create or replace function public.place_cloud_work(p_workspace_id uuid, p_profile text, p_mode text, p_target integer, p_memory_mb integer,
    p_idempotency_key uuid, p_location text default 'any', p_inference_node_id text default null, p_inference_model text default null)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare plan jsonb; node text; prior public.work_items;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    select * into prior from public.work_items where workspace_id = p_workspace_id and requester_id = auth.uid() and idempotency_key = p_idempotency_key;
    if found then
        if (prior.profile, prior.mode, prior.target, prior.memory_mb, prior.inference_node_id, prior.inference_model)
            is distinct from (p_profile, p_mode, p_target, p_memory_mb, p_inference_node_id, p_inference_model) then
            raise exception 'idempotency_conflict' using errcode = '22023'; end if;
        return jsonb_build_object('work_id', prior.work_id, 'node_id', prior.node_id, 'queued_reason', null, 'duplicate', true);
    end if;
    plan := gah_private.placement(p_workspace_id, p_profile, p_mode, p_target, p_memory_mb, p_location, p_inference_node_id, p_inference_model);
    node := plan->>'chosen';
    if node is null then raise exception 'unroutable' using errcode = '55000', detail = plan::text; end if;
    return jsonb_build_object('work_id', public.enqueue_cloud_work(p_workspace_id, node, p_profile, p_mode, p_target, p_memory_mb, p_idempotency_key, p_inference_node_id, p_inference_model),
        'node_id', node, 'queued_reason', (select c->>'queued_reason' from jsonb_array_elements(plan->'candidates') c where c->>'node_id' = node));
end;
$$;

-- The requester retries a run that ended without success, on the same or another worker.
create or replace function public.retry_cloud_work(p_workspace_id uuid, p_work_id text, p_node_id text default null, p_acknowledge_unknown_outcome boolean default false)
returns text language plpgsql security definer set search_path = '' as $$
declare w public.work_items; fresh text;
begin
    select * into w from public.work_items where workspace_id = p_workspace_id and work_id = p_work_id and requester_id = auth.uid() for update;
    if not found or not gah_private.can_access(p_workspace_id) then raise exception 'retry_denied' using errcode = '42501'; end if;
    if w.state not in ('failed', 'cancelled') then raise exception 'not_retryable' using errcode = '55000'; end if;
    if w.started_at is not null and w.exit_code is null then
        if p_acknowledge_unknown_outcome is not true then raise exception 'unknown_outcome' using errcode = '55000'; end if;
        if exists (select 1 from public.file_locks where workspace_id = w.workspace_id and work_id = w.work_id and resolved_at is null) then
            raise exception 'locks_unresolved' using errcode = '55006'; end if;
    end if;
    fresh := public.enqueue_cloud_work(w.workspace_id, coalesce(p_node_id, w.node_id), w.profile, w.mode, w.target, w.memory_mb, gen_random_uuid(), w.inference_node_id, w.inference_model);
    update public.work_items set retry_of = w.work_id where workspace_id = w.workspace_id and work_id = fresh;
    insert into public.events (workspace_id, event_id, work_id, kind, actor_id, payload)
    values (w.workspace_id, gen_random_uuid()::text, fresh, 'run.retried', auth.uid(),
        jsonb_build_object('retry_of', w.work_id, 'moved', coalesce(p_node_id, w.node_id) is distinct from w.node_id, 'unknown_outcome_acknowledged', w.started_at is not null and w.exit_code is null));
    return fresh;
end;
$$;

revoke all on function public.explain_cloud_placement(uuid, text, text, integer, integer, text, text, text), public.place_cloud_work(uuid, text, text, integer, integer, uuid, text, text, text),
    public.retry_cloud_work(uuid, text, text, boolean) from public, anon;
grant execute on function public.explain_cloud_placement(uuid, text, text, integer, integer, text, text, text), public.place_cloud_work(uuid, text, text, integer, integer, uuid, text, text, text),
    public.retry_cloud_work(uuid, text, text, boolean) to authenticated;

commit;
