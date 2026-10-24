-- ORG-02: every run and event names its initiating member, device, and agent session, all derived on the server.
-- claim_id stays the secret bearer for run mutations. agent_session_id is its public name and is safe to show coworkers.
begin;

alter table public.work_items add column if not exists agent_session_id uuid;
alter table public.events add column if not exists actor_id uuid;
alter table public.events add column if not exists device_id uuid;
alter table public.events add column if not exists agent_session_id uuid;

-- Callers never supply attribution: it is copied from the work row the server just changed.
create or replace function gah_private.audit(w public.work_items, p_kind text)
returns void language sql security definer set search_path = '' as $$
    insert into public.events (workspace_id, event_id, work_id, kind, actor_id, device_id, agent_session_id)
    values (w.workspace_id, gen_random_uuid()::text, w.work_id, p_kind, w.requester_id, w.device_id, w.agent_session_id);
$$;
revoke all on function gah_private.audit(public.work_items, text) from public, anon, authenticated;

-- Each claim is a new agent session, so two agents sharing one device token hold different identities.
create or replace function public.claim_cloud_work(p_device_token text, p_available_memory_mb integer, p_profiles text[], p_allow_inference boolean)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); n public.nodes; w public.work_items; active_count integer; reserved bigint;
begin
    select * into n from public.nodes where workspace_id = d.workspace_id and node_id = d.node_id for update;
    -- A lost running lease is terminal. Never automatically execute it twice.
    update public.work_items set state = case when started_at is null then 'queued' else 'failed' end,
        claim_id = case when started_at is null then null else claim_id end,
        device_id = case when started_at is null then null else device_id end,
        agent_session_id = case when started_at is null then null else agent_session_id end,
        leased_until = null, updated_at = statement_timestamp()
    where workspace_id = d.workspace_id and node_id = d.node_id and state = 'running' and leased_until <= statement_timestamp();
    update public.work_items x set state = 'cancelled', updated_at = statement_timestamp()
    where x.workspace_id = d.workspace_id and x.node_id = d.node_id and x.state = 'queued' and not gah_private.job_allowed(x);
    select count(*), coalesce(sum(memory_mb), 0) into active_count, reserved from public.work_items
    where workspace_id = d.workspace_id and node_id = d.node_id and state = 'running';
    if active_count >= n.max_concurrent then return null; end if;
    select * into w from public.work_items x where x.workspace_id = d.workspace_id and x.node_id = d.node_id
        and x.state = 'queued' and x.profile = any(p_profiles) and gah_private.job_allowed(x)
        and x.memory_mb <= greatest(0, least(n.memory_budget_mb - reserved, p_available_memory_mb))
        and (p_allow_inference or x.inference_node_id is null)
    order by x.updated_at, x.work_id limit 1 for update;
    if not found then return null; end if;
    update public.work_items set state = 'running', device_id = d.id, claim_id = gen_random_uuid(), agent_session_id = gen_random_uuid(),
        leased_until = statement_timestamp() + interval '30 seconds', updated_at = statement_timestamp()
    where workspace_id = w.workspace_id and work_id = w.work_id returning * into w;
    perform gah_private.audit(w, 'cloud.claimed');
    return to_jsonb(w);
end;
$$;

-- The release event keeps the releasing device and session even though the row no longer holds them.
create or replace function public.release_cloud_work(p_device_token text, p_work_id text, p_claim_id uuid)
returns boolean language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; held public.work_items;
begin
    select * into held from public.work_items where workspace_id = d.workspace_id and node_id = d.node_id and work_id = p_work_id
        and device_id = d.id and claim_id = p_claim_id and state = 'running' and started_at is null for update;
    if not found then return false; end if;
    update public.work_items set state = 'queued', device_id = null, claim_id = null, agent_session_id = null,
        leased_until = null, updated_at = statement_timestamp()
    where workspace_id = held.workspace_id and work_id = held.work_id returning * into w;
    w.device_id := held.device_id; w.agent_session_id := held.agent_session_id;
    perform gah_private.audit(w, 'cloud.capacity_deferred'); return true;
end;
$$;

commit;
