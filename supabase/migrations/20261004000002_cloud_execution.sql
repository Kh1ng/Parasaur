-- Outbound workers authenticate with revocable, node-bound credentials.
-- The payment gate and resource grants are checked again at every transition.
begin;

alter table public.nodes add column if not exists inference_scope text not null default 'private'
    check (inference_scope in ('private', 'workspace', 'selected'));
alter table public.nodes add column if not exists max_concurrent integer not null default 1 check (max_concurrent between 1 and 32);
alter table public.nodes add column if not exists memory_budget_mb integer not null default 4096 check (memory_budget_mb between 64 and 1048576);
alter table public.nodes add column if not exists inference_models text[] not null default '{}'
    check (cardinality(inference_models) <= 100 and octet_length(inference_models::text) <= 16000);
alter table gah_private.node_grants add column if not exists resource text not null default 'compute'
    check (resource in ('compute', 'inference'));
alter table gah_private.node_grants drop constraint if exists node_grants_pkey;
alter table gah_private.node_grants add primary key (workspace_id, node_id, user_id, resource);

create or replace function gah_private.member_active(p_workspace uuid, p_user uuid)
returns boolean language sql stable security definer set search_path = '' as $$
    select p_user is not null and exists (select 1 from gah_private.memberships m
        where m.workspace_id = p_workspace and m.user_id = p_user)
    and exists (select 1 from gah_private.deployment d where not d.requires_entitlement
        or exists (select 1 from gah_private.entitlements e where e.user_id = p_user
            and e.revoked_at is null and e.valid_until > statement_timestamp()));
$$;
create or replace function gah_private.resource_permission(p_workspace uuid, p_node text, p_resource text, p_user uuid)
returns text language sql stable security definer set search_path = '' as $$
    select case when n.owner_id = p_user then 'owner'
        when (case p_resource when 'compute' then n.sharing_scope else n.inference_scope end) = 'workspace' then 'use'
        when (case p_resource when 'compute' then n.sharing_scope else n.inference_scope end) = 'selected' then g.permission end
    from public.nodes n left join gah_private.node_grants g on g.workspace_id = n.workspace_id
        and g.node_id = n.node_id and g.user_id = p_user and g.resource = p_resource
    where n.workspace_id = p_workspace and n.node_id = p_node and p_resource in ('compute', 'inference')
        and gah_private.member_active(p_workspace, p_user)
        and gah_private.member_active(p_workspace, n.owner_id)
        and (n.revoked_at is null or n.owner_id = p_user);
$$;
create or replace function gah_private.node_permission(p_workspace_id uuid, p_node_id text)
returns text language sql stable security definer set search_path = '' as $$
    select gah_private.resource_permission(p_workspace_id, p_node_id, 'compute', auth.uid());
$$;
revoke all on function gah_private.member_active(uuid, uuid), gah_private.resource_permission(uuid, text, text, uuid) from public, anon, authenticated;

-- Preserve the original compute-only API while giving inference its own grants.
create or replace function public.set_cloud_node_grant(p_workspace_id uuid, p_node_id text, p_user_id uuid, p_permission text)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if gah_private.node_permission(p_workspace_id, p_node_id) is distinct from 'owner' then
        raise exception 'node_write_denied' using errcode = '42501';
    end if;
    if p_permission is null or p_permission not in ('use', 'manage', 'remove') then
        raise exception 'invalid_node_permission' using errcode = '22023';
    end if;
    if not exists (select 1 from gah_private.memberships where workspace_id = p_workspace_id and user_id = p_user_id) then
        raise exception 'workspace_member_required' using errcode = '42501';
    end if;
    if p_permission = 'remove' then
        delete from gah_private.node_grants where workspace_id = p_workspace_id and node_id = p_node_id and user_id = p_user_id and resource = 'compute';
    else
        insert into gah_private.node_grants (workspace_id, node_id, user_id, permission, resource)
        values (p_workspace_id, p_node_id, p_user_id, p_permission, 'compute')
        on conflict (workspace_id, node_id, user_id, resource) do update set permission = excluded.permission;
    end if;
end;
$$;
create or replace function public.cloud_node_grants(p_workspace_id uuid, p_node_id text)
returns table (user_id uuid, permission text) language sql stable security definer set search_path = '' as $$
    select g.user_id, g.permission from gah_private.node_grants g where g.workspace_id = p_workspace_id
    and g.node_id = p_node_id and g.resource = 'compute' and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
$$;

create or replace function public.configure_cloud_inference(p_workspace_id uuid, p_node_id text, p_scope text, p_models text[])
returns void language plpgsql security definer set search_path = '' as $$
begin
    if p_scope is null or p_scope not in ('private', 'workspace', 'selected') then
        raise exception 'invalid_inference_scope' using errcode = '22023';
    end if;
    update public.nodes set inference_scope = p_scope, inference_models = p_models
    where workspace_id = p_workspace_id and node_id = p_node_id and revoked_at is null
        and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
    if not found then raise exception 'node_write_denied' using errcode = '42501'; end if;
end;
$$;
create or replace function public.configure_cloud_capacity(p_workspace_id uuid, p_node_id text, p_max_concurrent integer, p_memory_budget_mb integer)
returns void language plpgsql security definer set search_path = '' as $$
begin
    update public.nodes set max_concurrent = p_max_concurrent, memory_budget_mb = p_memory_budget_mb
    where workspace_id = p_workspace_id and node_id = p_node_id and revoked_at is null
        and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
    if not found then raise exception 'node_write_denied' using errcode = '42501'; end if;
end;
$$;
create or replace function public.set_cloud_inference_grant(p_workspace_id uuid, p_node_id text, p_user_id uuid, p_permission text)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if gah_private.node_permission(p_workspace_id, p_node_id) is distinct from 'owner' then
        raise exception 'node_write_denied' using errcode = '42501';
    end if;
    if p_permission is null or p_permission not in ('use', 'remove') then
        raise exception 'invalid_inference_permission' using errcode = '22023';
    end if;
    if p_permission = 'remove' then
        delete from gah_private.node_grants where workspace_id = p_workspace_id and node_id = p_node_id and user_id = p_user_id and resource = 'inference';
    else
        insert into gah_private.node_grants (workspace_id, node_id, user_id, permission, resource)
        values (p_workspace_id, p_node_id, p_user_id, p_permission, 'inference')
        on conflict (workspace_id, node_id, user_id, resource) do update set permission = excluded.permission;
    end if;
end;
$$;
create or replace function public.cloud_inference_grants(p_workspace_id uuid, p_node_id text)
returns table (user_id uuid, permission text) language sql stable security definer set search_path = '' as $$
    select g.user_id, g.permission from gah_private.node_grants g where g.workspace_id = p_workspace_id
        and g.node_id = p_node_id and g.resource = 'inference' and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
$$;
create or replace function public.cloud_resource_permissions(p_workspace_id uuid)
returns table (node_id text, compute_permission text, inference_permission text)
language sql stable security definer set search_path = '' as $$
    select n.node_id, gah_private.node_permission(n.workspace_id, n.node_id),
        gah_private.resource_permission(n.workspace_id, n.node_id, 'inference', auth.uid())
    from public.nodes n where n.workspace_id = p_workspace_id and (gah_private.node_permission(n.workspace_id, n.node_id) is not null
        or gah_private.resource_permission(n.workspace_id, n.node_id, 'inference', auth.uid()) is not null);
$$;
drop policy if exists cloud_nodes_read on public.nodes;
create policy cloud_nodes_read on public.nodes for select to authenticated using (
    gah_private.node_permission(workspace_id, node_id) is not null
    or gah_private.resource_permission(workspace_id, node_id, 'inference', auth.uid()) is not null);
-- Policy helpers expose only an authorization decision, never private records.
grant execute on function gah_private.resource_permission(uuid, text, text, uuid) to authenticated;

create table if not exists gah_private.devices (
    id uuid primary key default gen_random_uuid(), workspace_id uuid not null, node_id text not null,
    token_hash text not null unique, revoked_at timestamptz, created_at timestamptz not null default now(),
    foreign key (workspace_id, node_id) references public.nodes on delete cascade
);
alter table gah_private.devices enable row level security;
revoke all on gah_private.devices from public, anon, authenticated;
grant all on gah_private.devices to service_role;

create or replace function public.enroll_cloud_device(p_workspace_id uuid, p_node_id text, p_max_concurrent integer, p_memory_budget_mb integer)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare credential text := gen_random_uuid()::text || gen_random_uuid()::text; device_id uuid;
begin
    update public.nodes set max_concurrent = p_max_concurrent, memory_budget_mb = p_memory_budget_mb
    where workspace_id = p_workspace_id and node_id = p_node_id and revoked_at is null
        and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
    if not found then raise exception 'node_write_denied' using errcode = '42501'; end if;
    insert into gah_private.devices (workspace_id, node_id, token_hash)
    values (p_workspace_id, p_node_id, encode(sha256(convert_to(credential, 'UTF8')), 'hex')) returning id into device_id;
    return jsonb_build_object('device_id', device_id, 'device_token', credential, 'workspace_id', p_workspace_id, 'node_id', p_node_id);
end;
$$;
create or replace function public.revoke_cloud_device(p_workspace_id uuid, p_device_id uuid)
returns void language plpgsql security definer set search_path = '' as $$
begin
    update gah_private.devices d set revoked_at = statement_timestamp() where d.id = p_device_id
        and d.workspace_id = p_workspace_id and gah_private.node_permission(d.workspace_id, d.node_id) = 'owner';
    if not found then raise exception 'device_write_denied' using errcode = '42501'; end if;
end;
$$;
create or replace function gah_private.device(p_token text)
returns gah_private.devices language plpgsql stable security definer set search_path = '' as $$
declare d gah_private.devices;
begin
    if p_token is null or length(p_token) <> 72 then raise exception 'device_access_denied' using errcode = '42501'; end if;
    select x.* into d from gah_private.devices x join public.nodes n on n.workspace_id = x.workspace_id and n.node_id = x.node_id
    where x.token_hash = encode(sha256(convert_to(p_token, 'UTF8')), 'hex') and x.revoked_at is null
        and n.revoked_at is null and gah_private.member_active(n.workspace_id, n.owner_id);
    if not found then raise exception 'device_access_denied' using errcode = '42501'; end if;
    return d;
end;
$$;
revoke all on function gah_private.device(text) from public, anon, authenticated;

alter table public.work_items add column if not exists requester_id uuid references auth.users;
alter table public.work_items add column if not exists idempotency_key uuid;
alter table public.work_items add column if not exists mode text check (mode in ('improve', 'review'));
alter table public.work_items add column if not exists target integer check (target > 0);
alter table public.work_items add column if not exists memory_mb integer check (memory_mb between 64 and 1048576);
alter table public.work_items add column if not exists inference_node_id text;
alter table public.work_items add column if not exists inference_model text;
alter table public.work_items add column if not exists device_id uuid references gah_private.devices;
alter table public.work_items add column if not exists claim_id uuid;
alter table public.work_items add column if not exists leased_until timestamptz;
alter table public.work_items add column if not exists started_at timestamptz;
alter table public.work_items add column if not exists exit_code integer;
create unique index if not exists cloud_work_idempotency on public.work_items (workspace_id, requester_id, idempotency_key);
create index if not exists cloud_work_queue on public.work_items (workspace_id, node_id, updated_at) where state = 'queued';
drop policy if exists cloud_work_read on public.work_items;
create policy cloud_work_read on public.work_items for select to authenticated using (gah_private.can_access(workspace_id)
    and (requester_id = auth.uid() or gah_private.node_permission(workspace_id, node_id) in ('owner', 'manage')));
drop policy if exists cloud_events_read on public.events;
create policy cloud_events_read on public.events for select to authenticated using (exists (select 1 from public.work_items w
    where w.workspace_id = events.workspace_id and w.work_id = events.work_id));

create or replace function gah_private.job_allowed(w public.work_items)
returns boolean language sql stable security definer set search_path = '' as $$
    select exists (select 1 from public.nodes n where n.workspace_id = w.workspace_id and n.node_id = w.node_id
        and n.revoked_at is null and w.profile = any(n.profiles) and w.memory_mb <= n.memory_budget_mb)
    and gah_private.resource_permission(w.workspace_id, w.node_id, 'compute', w.requester_id) is not null
    and (w.device_id is null or exists (select 1 from gah_private.devices d where d.id = w.device_id and d.revoked_at is null))
    and (w.inference_node_id is null or exists (select 1 from public.nodes i where i.workspace_id = w.workspace_id
        and i.node_id = w.inference_node_id and i.revoked_at is null and w.inference_model = any(i.inference_models)
        and gah_private.resource_permission(w.workspace_id, w.inference_node_id, 'inference', w.requester_id) is not null));
$$;
revoke all on function gah_private.job_allowed(public.work_items) from public, anon, authenticated;
create or replace function gah_private.audit(w public.work_items, p_kind text)
returns void language sql security definer set search_path = '' as $$
    insert into public.events (workspace_id, event_id, work_id, kind)
    values (w.workspace_id, gen_random_uuid()::text, w.work_id, p_kind);
$$;
revoke all on function gah_private.audit(public.work_items, text) from public, anon, authenticated;

create or replace function public.enqueue_cloud_work(p_workspace_id uuid, p_node_id text, p_profile text,
    p_mode text, p_target integer, p_memory_mb integer, p_idempotency_key uuid,
    p_inference_node_id text default null, p_inference_model text default null)
returns text language plpgsql security definer set search_path = '' as $$
declare w public.work_items; prior public.work_items;
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
    insert into public.work_items (workspace_id, work_id, node_id, requester_id, profile, mode, target, memory_mb,
        idempotency_key, state, inference_node_id, inference_model)
    values (w.workspace_id, w.work_id, w.node_id, w.requester_id, w.profile, w.mode, w.target, w.memory_mb,
        w.idempotency_key, w.state, w.inference_node_id, w.inference_model);
    perform gah_private.audit(w, 'cloud.queued');
    return w.work_id;
end;
$$;

create or replace function public.cancel_cloud_work(p_workspace_id uuid, p_work_id text)
returns void language plpgsql security definer set search_path = '' as $$
declare w public.work_items;
begin
    update public.work_items set state = 'cancelled', leased_until = null, updated_at = statement_timestamp()
    where workspace_id = p_workspace_id and work_id = p_work_id and state in ('queued', 'running')
        and gah_private.can_access(workspace_id) and (requester_id = auth.uid() or gah_private.node_permission(workspace_id, node_id) in ('owner', 'manage')) returning * into w;
    if not found then raise exception 'work_write_denied' using errcode = '42501'; end if;
    perform gah_private.audit(w, 'cloud.cancelled');
end;
$$;

create or replace function public.claim_cloud_work(p_device_token text, p_available_memory_mb integer, p_profiles text[], p_allow_inference boolean)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); n public.nodes; w public.work_items; active_count integer; reserved bigint;
begin
    select * into n from public.nodes where workspace_id = d.workspace_id and node_id = d.node_id for update;
    -- A lost running lease is terminal. Never automatically execute it twice.
    update public.work_items set state = case when started_at is null then 'queued' else 'failed' end,
        claim_id = case when started_at is null then null else claim_id end,
        device_id = case when started_at is null then null else device_id end,
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
    update public.work_items set state = 'running', device_id = d.id, claim_id = gen_random_uuid(),
        leased_until = statement_timestamp() + interval '30 seconds', updated_at = statement_timestamp()
    where workspace_id = w.workspace_id and work_id = w.work_id returning * into w;
    perform gah_private.audit(w, 'cloud.claimed');
    return to_jsonb(w);
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
    if not gah_private.job_allowed(w) then
        update public.work_items set state = 'cancelled', leased_until = null, updated_at = statement_timestamp()
        where workspace_id = w.workspace_id and work_id = w.work_id;
        perform gah_private.audit(w, 'cloud.access_revoked'); return false;
    end if;
    if p_start and w.started_at is not null then return false; end if;
    update public.work_items set started_at = case when p_start then statement_timestamp() else started_at end,
        leased_until = statement_timestamp() + interval '30 seconds', updated_at = statement_timestamp()
    where workspace_id = w.workspace_id and work_id = w.work_id;
    if p_start then perform gah_private.audit(w, 'cloud.started'); end if;
    return true;
end;
$$;
create or replace function public.finish_cloud_work(p_device_token text, p_work_id text, p_claim_id uuid, p_exit_code integer)
returns boolean language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items;
begin
    update public.work_items x set state = case when not gah_private.job_allowed(x) then 'cancelled' when p_exit_code = 0 then 'completed' else 'failed' end,
        exit_code = p_exit_code, leased_until = null, updated_at = statement_timestamp()
    where x.workspace_id = d.workspace_id and x.node_id = d.node_id and x.work_id = p_work_id and x.device_id = d.id
        and x.claim_id = p_claim_id and x.state = 'running' and x.leased_until > statement_timestamp()
        and (x.started_at is not null or p_exit_code <> 0) returning * into w;
    if not found then return false; end if;
    perform gah_private.audit(w, 'cloud.' || w.state); return true;
end;
$$;
create or replace function public.release_cloud_work(p_device_token text, p_work_id text, p_claim_id uuid)
returns boolean language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items;
begin
    update public.work_items set state = 'queued', device_id = null, claim_id = null,
        leased_until = null, updated_at = statement_timestamp()
    where workspace_id = d.workspace_id and node_id = d.node_id and work_id = p_work_id
        and device_id = d.id and claim_id = p_claim_id and state = 'running' and started_at is null returning * into w;
    if not found then return false; end if;
    perform gah_private.audit(w, 'cloud.capacity_deferred'); return true;
end;
$$;
create or replace function public.heartbeat_cloud_device(p_device_token text)
returns void language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token);
begin
    update public.nodes set last_seen_at = statement_timestamp() where workspace_id = d.workspace_id and node_id = d.node_id;
end;
$$;

-- Inference payloads stay out of public tables and audit events. Only the two
-- bound devices can read them. Provider secrets are never accepted here.
create table if not exists gah_private.inference_requests (
    id uuid primary key default gen_random_uuid(), workspace_id uuid not null, work_id text not null,
    inference_node_id text not null, compute_device_id uuid not null references gah_private.devices,
    inference_device_id uuid references gah_private.devices, claim_id uuid,
    request_key uuid not null, path text not null check (path in ('chat/completions', 'responses')),
    body jsonb not null check (octet_length(body::text) <= 262144), response text check (octet_length(response) <= 1048576),
    status integer, state text not null default 'queued' check (state in ('queued', 'running', 'completed', 'failed')),
    expires_at timestamptz not null default now() + interval '120 seconds',
    unique (compute_device_id, work_id, request_key),
    foreign key (workspace_id, work_id) references public.work_items on delete cascade
);
alter table gah_private.inference_requests enable row level security;
revoke all on gah_private.inference_requests from public, anon, authenticated;
grant all on gah_private.inference_requests to service_role;
create or replace function public.enqueue_cloud_inference(p_device_token text, p_work_id text, p_claim_id uuid,
    p_request_key uuid, p_path text, p_body jsonb)
returns uuid language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; request_id uuid;
begin
    select * into w from public.work_items where workspace_id = d.workspace_id and work_id = p_work_id
        and node_id = d.node_id and device_id = d.id and claim_id = p_claim_id and state = 'running'
        and started_at is not null and leased_until > statement_timestamp() for update;
    if not found or w.inference_node_id is null or not gah_private.job_allowed(w) then
        raise exception 'inference_access_denied' using errcode = '42501';
    end if;
    if jsonb_typeof(p_body) is distinct from 'object' or p_body->>'model' is distinct from w.inference_model
        or p_path is null or p_path not in ('chat/completions', 'responses') or p_request_key is null
        or octet_length(p_body::text) > 262144 then raise exception 'invalid_inference_request' using errcode = '22023'; end if;
    delete from gah_private.inference_requests where expires_at < statement_timestamp();
    if (select count(*) from gah_private.inference_requests where work_id = w.work_id and workspace_id = w.workspace_id and state in ('queued','running')) >= 8 then
        raise exception 'inference_queue_limit' using errcode = '54000'; end if;
    insert into gah_private.inference_requests (workspace_id, work_id, inference_node_id, compute_device_id, request_key, path, body)
    values (w.workspace_id, w.work_id, w.inference_node_id, d.id, p_request_key, p_path, p_body)
    on conflict (compute_device_id, work_id, request_key) do nothing returning id into request_id;
    if request_id is null then
        select id into request_id from gah_private.inference_requests where compute_device_id = d.id and work_id = w.work_id and request_key = p_request_key and path = p_path and body = p_body;
        if request_id is null then raise exception 'idempotency_conflict' using errcode = '22023'; end if;
    end if;
    return request_id;
end;
$$;
create or replace function public.claim_cloud_inference(p_device_token text)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); r gah_private.inference_requests;
begin
    perform 1 from public.nodes where workspace_id = d.workspace_id and node_id = d.node_id for update;
    if (select count(*) from gah_private.inference_requests where workspace_id = d.workspace_id and inference_node_id = d.node_id and state = 'running' and expires_at > statement_timestamp())
        >= (select max_concurrent from public.nodes where workspace_id = d.workspace_id and node_id = d.node_id) then return null; end if;
    select x.* into r from gah_private.inference_requests x join public.work_items w on w.workspace_id = x.workspace_id and w.work_id = x.work_id
    where x.workspace_id = d.workspace_id and x.inference_node_id = d.node_id and x.state = 'queued' and x.expires_at > statement_timestamp()
        and w.state = 'running' and w.leased_until > statement_timestamp() and gah_private.job_allowed(w)
    order by x.expires_at limit 1 for update of x skip locked;
    if not found then return null; end if;
    update gah_private.inference_requests set state = 'running', inference_device_id = d.id, claim_id = gen_random_uuid()
    where id = r.id returning * into r;
    return jsonb_build_object('id', r.id, 'claim_id', r.claim_id, 'path', r.path, 'body', r.body);
end;
$$;
create or replace function public.check_cloud_inference(p_device_token text, p_request_id uuid, p_claim_id uuid)
returns boolean language sql stable security definer set search_path = '' as $$
    select exists (select 1 from gah_private.inference_requests r join public.work_items w on w.workspace_id = r.workspace_id and w.work_id = r.work_id
        where r.id = p_request_id and r.inference_device_id = (gah_private.device(p_device_token)).id
        and r.claim_id = p_claim_id and r.state = 'running' and r.expires_at > statement_timestamp()
        and w.state = 'running' and w.leased_until > statement_timestamp() and gah_private.job_allowed(w));
$$;
create or replace function public.finish_cloud_inference(p_device_token text, p_request_id uuid, p_claim_id uuid, p_status integer, p_response text)
returns boolean language plpgsql security definer set search_path = '' as $$
begin
    if not public.check_cloud_inference(p_device_token, p_request_id, p_claim_id) then return false; end if;
    if p_status is null or p_status not between 100 and 599 or p_response is null or octet_length(p_response) > 1048576 then
        raise exception 'invalid_inference_response' using errcode = '22023'; end if;
    update gah_private.inference_requests set state = case when p_status between 200 and 299 then 'completed' else 'failed' end,
        status = p_status, response = p_response, body = '{}'::jsonb where id = p_request_id;
    return true;
end;
$$;
create or replace function public.poll_cloud_inference(p_device_token text, p_request_id uuid)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); r gah_private.inference_requests; w public.work_items;
begin
    select * into r from gah_private.inference_requests where id = p_request_id and compute_device_id = d.id and expires_at > statement_timestamp();
    if not found then raise exception 'inference_access_denied' using errcode = '42501'; end if;
    select * into w from public.work_items where workspace_id = r.workspace_id and work_id = r.work_id;
    if w.state <> 'running' or w.leased_until <= statement_timestamp() or not gah_private.job_allowed(w) then
        raise exception 'inference_access_denied' using errcode = '42501'; end if;
    if r.state in ('completed', 'failed') then
        delete from gah_private.inference_requests where id = r.id;
        return jsonb_build_object('status', r.status, 'response', r.response);
    end if;
    return null;
end;
$$;

-- Explicit allowlists avoid platform default EXECUTE grants to PUBLIC.
do $$
declare f record;
begin
    for f in select p.oid::regprocedure as signature from pg_proc p join pg_namespace s on s.oid = p.pronamespace
        where s.nspname = 'public' and p.proname in ('configure_cloud_inference','configure_cloud_capacity','set_cloud_inference_grant',
        'cloud_inference_grants','cloud_resource_permissions','enroll_cloud_device','revoke_cloud_device','enqueue_cloud_work','cancel_cloud_work')
    loop
        execute format('revoke all on function %s from public, anon, authenticated, service_role', f.signature);
        execute format('grant execute on function %s to authenticated', f.signature);
    end loop;
    for f in select p.oid::regprocedure as signature from pg_proc p join pg_namespace s on s.oid = p.pronamespace
        where s.nspname = 'public' and p.proname in ('claim_cloud_work','renew_cloud_work','finish_cloud_work','release_cloud_work','heartbeat_cloud_device',
        'enqueue_cloud_inference','claim_cloud_inference','check_cloud_inference','finish_cloud_inference','poll_cloud_inference')
    loop
        execute format('revoke all on function %s from public, anon, authenticated, service_role', f.signature);
        execute format('grant execute on function %s to anon, authenticated', f.signature);
    end loop;
end;
$$;
commit;
