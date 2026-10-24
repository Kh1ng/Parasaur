-- ORG-11 (control plane): lifecycle records for managed cloud workers, independent of any provider.
-- A separate operator service (service role) holds the provider account and performs the real provisioning. This
-- migration is what that service reads and writes, and what keeps it honest: no duplicate workers for a retried request,
-- no paid resource without a record, no retirement before cleanup is confirmed, and credentials that die with the worker.
-- States: requested, starting, ready, busy, stopping, failed, retired.
--   requested -> starting -> ready <-> busy; any live state -> stopping; any live state -> failed -> stopping; stopping -> retired.
-- Rules:
--   Admission: managed access must be enabled by an owner (ORG-14), the caller must be allowed to launch paid work, and the
--     count of live workers must stay within the workspace concurrency limit. A retried request with the same key returns the
--     same worker, so provisioning retries cannot create a second one.
--   Tracking: the operator records provider_ref the moment a resource exists. A worker with a provider_ref cannot retire until
--     cleanup is confirmed with an artifact disposition, so a crash mid-cleanup leaves it visible to reconcile, not forgotten.
--   Credentials: the device token is created at bootstrap, handed to the operator once, and never stored in plain text or in
--     an image. Entering stopping or failed revokes it, and retiring revokes the node.
--   Cancellation: a worker still requested with no resource retires at once; otherwise (starting or later) it moves to
--     stopping, because the operator may already have created a resource it has not recorded yet. The operator can still
--     record provider_ref on a stopping worker (stopping -> stopping) so cleanup is tracked.
--   Crash, idle: reconcile fails a ready or busy worker whose heartbeat has been silent, derives busy from running work, and
--     moves workers idle past their limit to stopping. It also lists workers stuck in requested, starting, or stopping.
-- Ownership: a managed node has no member owner (owner_id is null). The requester is recorded in managed_workers.requested_by
--   only, so no member can enroll devices on it, re-register it, change its sharing or grants, or take over its runs as node
--   owner; it is a workspace resource usable by active members under ORG-14 admission. Removing the requester still stops
--   the worker at the next reconcile.
-- Provider, region, and runtime pinning are recorded as given; choosing them is the operator's decision (Q7/Q8).
begin;

create table if not exists gah_private.managed_workers (
    workspace_id uuid not null references public.workspaces on delete cascade,
    worker_id uuid not null default gen_random_uuid(),
    request_key uuid not null,
    requested_by uuid,
    lifetime text not null check (lifetime in ('task', 'persistent')),
    idle_minutes integer not null check (idle_minutes between 1 and 1440),
    profiles text[] not null check (cardinality(profiles) between 1 and 20),
    memory_mb integer not null default 4096 check (memory_mb between 64 and 1048576),
    runtime_version text not null check (length(runtime_version) between 1 and 64),
    state text not null default 'requested' check (state in ('requested', 'starting', 'ready', 'busy', 'stopping', 'failed', 'retired')),
    provider text, region text, provider_ref text check (length(provider_ref) <= 512),
    node_id text, device_id uuid,
    cleanup_confirmed boolean not null default false,
    artifacts text check (artifacts in ('preserved', 'discarded')),
    last_error text check (length(last_error) <= 500),
    created_at timestamptz not null default now(), updated_at timestamptz not null default now(), idle_since timestamptz,
    primary key (workspace_id, worker_id),
    unique (workspace_id, request_key)
);
alter table gah_private.managed_workers alter column requested_by drop not null;
alter table gah_private.managed_workers enable row level security;
revoke all on gah_private.managed_workers from public, anon, authenticated;
grant all on gah_private.managed_workers to service_role;

create or replace function public.request_managed_worker(p_workspace_id uuid, p_request_key uuid, p_lifetime text, p_runtime_version text,
    p_profiles text[], p_memory_mb integer default 4096, p_idle_minutes integer default null)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare l gah_private.cloud_limits; w gah_private.managed_workers; live integer;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    select * into l from gah_private.cloud_limits where workspace_id = p_workspace_id for update;
    if not found or not l.managed_enabled then raise exception 'managed_not_enabled' using errcode = '42501'; end if;
    if not (l.members_may_launch or gah_private.workspace_owner(p_workspace_id)) then raise exception 'launch_not_permitted' using errcode = '42501'; end if;
    select * into w from gah_private.managed_workers where workspace_id = p_workspace_id and request_key = p_request_key;
    if found then
        if w.requested_by is distinct from auth.uid() or (w.lifetime, w.runtime_version, w.profiles) is distinct from (p_lifetime, p_runtime_version, p_profiles) then
            raise exception 'idempotency_conflict' using errcode = '22023'; end if;
        return to_jsonb(w) - 'provider_ref' - 'last_error';
    end if;
    if p_lifetime not in ('task', 'persistent') or p_runtime_version is null or p_profiles is null or cardinality(p_profiles) not between 1 and 20 then
        raise exception 'invalid_worker_request' using errcode = '22023'; end if;
    select count(*) into live from gah_private.managed_workers where workspace_id = p_workspace_id and state <> 'retired';
    if live >= l.managed_max_concurrent then raise exception 'concurrency_limit' using errcode = '53400'; end if;
    insert into gah_private.managed_workers (workspace_id, request_key, requested_by, lifetime, idle_minutes, profiles, memory_mb, runtime_version)
    values (p_workspace_id, p_request_key, auth.uid(), p_lifetime, coalesce(p_idle_minutes, case p_lifetime when 'task' then 2 else 60 end), p_profiles, p_memory_mb, p_runtime_version)
    returning * into w;
    return to_jsonb(w) - 'provider_ref' - 'last_error';
end;
$$;

-- A requester or workspace owner cancels. With no resource there is nothing to clean; otherwise cleanup follows.
create or replace function public.cancel_managed_worker(p_workspace_id uuid, p_worker_id uuid)
returns text language plpgsql security definer set search_path = '' as $$
declare w gah_private.managed_workers;
begin
    select * into w from gah_private.managed_workers where workspace_id = p_workspace_id and worker_id = p_worker_id for update;
    if not found or not gah_private.can_access(p_workspace_id) or not (w.requested_by = auth.uid() or gah_private.workspace_owner(p_workspace_id)) then
        raise exception 'worker_write_denied' using errcode = '42501'; end if;
    if w.state in ('stopping', 'retired') then return w.state; end if;
    update gah_private.managed_workers set state = case when state = 'requested' and provider_ref is null and device_id is null then 'retired' else 'stopping' end,
        cleanup_confirmed = state = 'requested' and provider_ref is null and device_id is null, updated_at = statement_timestamp()
    where workspace_id = p_workspace_id and worker_id = p_worker_id returning state into w.state;
    if w.device_id is not null then update gah_private.devices set revoked_at = coalesce(revoked_at, statement_timestamp()) where id = w.device_id; end if;
    return w.state;
end;
$$;

create or replace function public.cloud_managed_workers(p_workspace_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    return coalesce((select jsonb_agg(to_jsonb(x) order by x.created_at desc) from (
        select worker_id, state, lifetime, node_id, runtime_version, region, created_at, updated_at, requested_by from gah_private.managed_workers
        where workspace_id = p_workspace_id and (state <> 'retired' or updated_at > statement_timestamp() - interval '7 days') order by created_at desc limit 100) x), '[]');
end;
$$;

-- Operator side (service role only).
create or replace function public.managed_worker_transition(p_workspace_id uuid, p_worker_id uuid, p_state text, p_provider text default null,
    p_region text default null, p_provider_ref text default null, p_error text default null)
returns void language plpgsql security definer set search_path = '' as $$
declare w gah_private.managed_workers; allowed boolean;
begin
    select * into w from gah_private.managed_workers where workspace_id = p_workspace_id and worker_id = p_worker_id for update;
    if not found then raise exception 'worker_not_found' using errcode = '22023'; end if;
    allowed := case w.state
        when 'requested' then p_state in ('starting', 'stopping', 'failed')
        when 'starting' then p_state in ('ready', 'stopping', 'failed')
        when 'ready' then p_state in ('busy', 'stopping', 'failed')
        when 'busy' then p_state in ('ready', 'stopping', 'failed')
        when 'failed' then p_state = 'stopping'
        when 'stopping' then p_state in ('stopping', 'retired')
        else false end;
    if not allowed then raise exception 'invalid_transition' using errcode = '22023', detail = w.state || ' to ' || coalesce(p_state, 'null'); end if;
    if p_state = 'retired' and (w.provider_ref is not null or p_provider_ref is not null) and not w.cleanup_confirmed then raise exception 'cleanup_unconfirmed' using errcode = '55000'; end if;
    if p_state = 'retired' and w.provider_ref is not null and w.artifacts is null then raise exception 'artifacts_unresolved' using errcode = '55000'; end if;
    update gah_private.managed_workers set state = p_state, provider = coalesce(p_provider, provider), region = coalesce(p_region, region),
        provider_ref = coalesce(p_provider_ref, provider_ref), last_error = coalesce(left(p_error, 500), last_error), updated_at = statement_timestamp(),
        idle_since = case when p_state = 'ready' then coalesce(idle_since, statement_timestamp()) else null end
    where workspace_id = p_workspace_id and worker_id = p_worker_id;
    if p_state in ('stopping', 'failed', 'retired') and w.device_id is not null then
        update gah_private.devices set revoked_at = coalesce(revoked_at, statement_timestamp()) where id = w.device_id; end if;
    if p_state = 'retired' and w.node_id is not null then
        update public.nodes set revoked_at = coalesce(revoked_at, statement_timestamp()) where workspace_id = p_workspace_id and node_id = w.node_id; end if;
end;
$$;

alter table public.nodes alter column owner_id drop not null;

-- A managed node (no owner) is usable by active members through its scopes; its devices stay valid without an owner.
-- Every owner-only action (enroll, register, manage, grants, dashboard, revoke device) needs node_permission 'owner',
-- which nobody has on a managed node, so they are refused. A workspace owner maps its repository aliases.
create or replace function gah_private.resource_permission(p_workspace uuid, p_node text, p_resource text, p_user uuid)
returns text language sql stable security definer set search_path = '' as $$
    select case when n.owner_id = p_user then 'owner'
        when (case p_resource when 'compute' then n.sharing_scope else n.inference_scope end) = 'workspace' then 'use'
        when (case p_resource when 'compute' then n.sharing_scope else n.inference_scope end) = 'selected' then g.permission end
    from public.nodes n left join gah_private.node_grants g on g.workspace_id = n.workspace_id
        and g.node_id = n.node_id and g.user_id = p_user and g.resource = p_resource
    where n.workspace_id = p_workspace and n.node_id = p_node and p_resource in ('compute', 'inference')
        and gah_private.member_active(p_workspace, p_user)
        and (n.worker_kind = 'managed' or gah_private.member_active(p_workspace, n.owner_id))
        and (n.revoked_at is null or n.owner_id = p_user);
$$;
create or replace function gah_private.device(p_token text)
returns gah_private.devices language plpgsql stable security definer set search_path = '' as $$
declare d gah_private.devices;
begin
    if p_token is null or length(p_token) <> 72 then raise exception 'device_access_denied' using errcode = '42501'; end if;
    select x.* into d from gah_private.devices x join public.nodes n on n.workspace_id = x.workspace_id and n.node_id = x.node_id
    where x.token_hash = encode(sha256(convert_to(p_token, 'UTF8')), 'hex') and x.revoked_at is null
        and n.revoked_at is null and (n.worker_kind = 'managed' or gah_private.member_active(n.workspace_id, n.owner_id));
    if not found then raise exception 'device_access_denied' using errcode = '42501'; end if;
    return d;
end;
$$;
create or replace function public.map_cloud_repository_alias(p_workspace_id uuid, p_node_id text, p_profile text, p_repo_id uuid, p_local_path text default null)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if gah_private.node_permission(p_workspace_id, p_node_id) is distinct from 'owner' and not (gah_private.workspace_owner(p_workspace_id)
        and exists (select 1 from public.nodes where workspace_id = p_workspace_id and node_id = p_node_id and worker_kind = 'managed')) then
        raise exception 'node_write_denied' using errcode = '42501'; end if;
    if not exists (select 1 from public.repositories where workspace_id = p_workspace_id and repo_id = p_repo_id) then
        raise exception 'repository_not_found' using errcode = '22023'; end if;
    insert into public.repository_aliases (workspace_id, node_id, profile, repo_id, local_path)
    values (p_workspace_id, p_node_id, p_profile, p_repo_id, p_local_path)
    on conflict (workspace_id, node_id, profile) do update set repo_id = excluded.repo_id, local_path = excluded.local_path;
end;
$$;

-- Erasure (ORG-10) also removes the departed member as the requester of managed workers and the owner of their nodes.
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
    update gah_private.managed_workers set requested_by = null where workspace_id = p_workspace_id and requested_by = p_user_id;
    update public.nodes set owner_id = null where workspace_id = p_workspace_id and owner_id = p_user_id;
    delete from gah_private.notifications where workspace_id = p_workspace_id and user_id = p_user_id;
    update gah_private.notifications set payload = payload - 'to' - 'from'
        where workspace_id = p_workspace_id and (payload->>'to' = p_user_id::text or payload->>'from' = p_user_id::text);
    update public.tasks set handoff_kind = null, handoff_to = null, handoff_expires_at = null
        where workspace_id = p_workspace_id and handoff_to = p_user_id;
    return jsonb_build_object('events', e, 'audit', a, 'runs', r);
end;
$$;

-- The device token is returned once and only its hash is stored, so it can go into the new machine's environment, not its image.
create or replace function public.managed_worker_bootstrap(p_workspace_id uuid, p_worker_id uuid)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare w gah_private.managed_workers; credential text := gen_random_uuid()::text || gen_random_uuid()::text; node text; dev uuid;
begin
    select * into w from gah_private.managed_workers where workspace_id = p_workspace_id and worker_id = p_worker_id for update;
    if not found or w.state <> 'starting' then raise exception 'invalid_transition' using errcode = '22023'; end if;
    if w.device_id is not null then raise exception 'already_bootstrapped' using errcode = '22023'; end if;
    node := 'managed-' || substr(w.worker_id::text, 1, 8);
    insert into public.nodes (workspace_id, node_id, owner_id, display_name, advertised_url, version, schema_digest, transport_mode, profiles, sharing_scope,
        max_concurrent, memory_budget_mb, worker_kind)
    values (p_workspace_id, node, null, 'Managed worker ' || substr(w.worker_id::text, 1, 8), 'https://managed.invalid', w.runtime_version, 'managed', 'authenticated_remote',
        w.profiles, 'workspace', 1, w.memory_mb, 'managed');
    update public.nodes set inference_scope = 'workspace' where workspace_id = p_workspace_id and node_id = node;
    insert into gah_private.devices (workspace_id, node_id, token_hash) values (p_workspace_id, node, encode(sha256(convert_to(credential, 'UTF8')), 'hex')) returning id into dev;
    update gah_private.managed_workers set node_id = node, device_id = dev, updated_at = statement_timestamp() where workspace_id = p_workspace_id and worker_id = p_worker_id;
    return jsonb_build_object('worker_id', p_worker_id, 'node_id', node, 'device_id', dev, 'device_token', credential);
end;
$$;

create or replace function public.managed_worker_confirm_cleanup(p_workspace_id uuid, p_worker_id uuid, p_artifacts text)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if p_artifacts is null or p_artifacts not in ('preserved', 'discarded') then raise exception 'invalid_disposition' using errcode = '22023'; end if;
    update gah_private.managed_workers set cleanup_confirmed = true, artifacts = p_artifacts, updated_at = statement_timestamp()
    where workspace_id = p_workspace_id and worker_id = p_worker_id and state in ('stopping', 'failed');
    if not found then raise exception 'invalid_transition' using errcode = '22023'; end if;
end;
$$;

-- One pass for the operator: derive busy and ready, fail silent workers, stop idle ones, and report what needs a human or
-- a provider call. Run it on a schedule.
create or replace function public.reconcile_managed_workers(p_silent_seconds integer default 300, p_stuck_minutes integer default 15)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare w gah_private.managed_workers; live integer; silent boolean; failed_ids jsonb := '[]'; stopped_ids jsonb := '[]';
begin
    for w in select * from gah_private.managed_workers where state in ('ready', 'busy') for update loop
        select count(*) into live from public.work_items where workspace_id = w.workspace_id and node_id = w.node_id
            and (state = 'queued' or (state = 'running' and leased_until > statement_timestamp()));
        select n.worker_heartbeat_at is null or n.worker_heartbeat_at <= statement_timestamp() - make_interval(secs => p_silent_seconds) into silent
            from public.nodes n where n.workspace_id = w.workspace_id and n.node_id = w.node_id;
        if silent and w.updated_at <= statement_timestamp() - make_interval(secs => p_silent_seconds) then
            perform public.managed_worker_transition(w.workspace_id, w.worker_id, 'failed', null, null, null, 'worker stopped reporting'); failed_ids := failed_ids || to_jsonb(w.worker_id);
        elsif not gah_private.member_active(w.workspace_id, w.requested_by) then
            perform public.managed_worker_transition(w.workspace_id, w.worker_id, 'stopping', null, null, null, 'requester no longer has access'); stopped_ids := stopped_ids || to_jsonb(w.worker_id);
        elsif live > 0 and w.state = 'ready' then
            update gah_private.managed_workers set state = 'busy', idle_since = null, updated_at = statement_timestamp() where workspace_id = w.workspace_id and worker_id = w.worker_id;
        elsif live = 0 and w.state = 'busy' then
            update gah_private.managed_workers set state = 'ready', idle_since = statement_timestamp(), updated_at = statement_timestamp() where workspace_id = w.workspace_id and worker_id = w.worker_id;
        elsif live = 0 and w.state = 'ready' and w.idle_since is not null and w.idle_since <= statement_timestamp() - make_interval(mins => w.idle_minutes) then
            perform public.managed_worker_transition(w.workspace_id, w.worker_id, 'stopping', null, null, null, 'idle limit reached'); stopped_ids := stopped_ids || to_jsonb(w.worker_id);
        end if;
    end loop;
    return jsonb_build_object('failed', failed_ids, 'stopping', stopped_ids,
        'needs_provisioning', coalesce((select jsonb_agg(worker_id) from gah_private.managed_workers where state = 'requested'), '[]'),
        'needs_cleanup', coalesce((select jsonb_agg(jsonb_build_object('worker_id', worker_id, 'provider_ref', provider_ref, 'provider', provider)) from gah_private.managed_workers
            where state in ('stopping', 'failed') and provider_ref is not null and not cleanup_confirmed), '[]'),
        'stuck', coalesce((select jsonb_agg(worker_id) from gah_private.managed_workers where state in ('requested', 'starting', 'stopping')
            and updated_at <= statement_timestamp() - make_interval(mins => p_stuck_minutes)), '[]'));
end;
$$;

revoke all on function public.request_managed_worker(uuid, uuid, text, text, text[], integer, integer), public.cancel_managed_worker(uuid, uuid), public.cloud_managed_workers(uuid) from public, anon;
grant execute on function public.request_managed_worker(uuid, uuid, text, text, text[], integer, integer), public.cancel_managed_worker(uuid, uuid), public.cloud_managed_workers(uuid) to authenticated;
revoke all on function public.managed_worker_transition(uuid, uuid, text, text, text, text, text), public.managed_worker_bootstrap(uuid, uuid),
    public.managed_worker_confirm_cleanup(uuid, uuid, text), public.reconcile_managed_workers(integer, integer) from public, anon, authenticated;
grant execute on function public.managed_worker_transition(uuid, uuid, text, text, text, text, text), public.managed_worker_bootstrap(uuid, uuid),
    public.managed_worker_confirm_cleanup(uuid, uuid, text), public.reconcile_managed_workers(integer, integer) to service_role;

commit;
