-- Managed access is a database decision, never a client-provided mode/plan.
begin;

create schema if not exists gah_private;
revoke all on schema gah_private from public, anon, authenticated;
grant usage on schema gah_private to authenticated, service_role;

create table if not exists gah_private.deployment (
    singleton boolean primary key default true check (singleton),
    requires_entitlement boolean not null default true
);
insert into gah_private.deployment (singleton) values (true) on conflict do nothing;

create table if not exists public.workspaces (
    id uuid primary key default gen_random_uuid(),
    name text not null check (length(name) between 1 and 100)
);
create table if not exists gah_private.memberships (
    workspace_id uuid not null references public.workspaces on delete cascade,
    user_id uuid not null references auth.users on delete cascade,
    display_name text not null default 'Member' check (length(display_name) between 1 and 100),
    primary key (workspace_id, user_id)
);
create table if not exists gah_private.entitlements (
    user_id uuid primary key references auth.users on delete cascade,
    valid_until timestamptz not null,
    revoked_at timestamptz
);

-- No caller can create memberships, grant a plan, or switch deployment policy.
alter table gah_private.deployment enable row level security;
alter table gah_private.memberships enable row level security;
alter table gah_private.entitlements enable row level security;
revoke all on all tables in schema gah_private from public, anon, authenticated;
grant all on all tables in schema gah_private to service_role;

create or replace function gah_private.can_access(p_workspace_id uuid)
returns boolean language sql stable security definer set search_path = '' as $$
    select auth.uid() is not null
       and exists (select 1 from gah_private.memberships m
                   where m.workspace_id = p_workspace_id and m.user_id = auth.uid())
       and exists (select 1 from gah_private.deployment d
                   where not d.requires_entitlement
                      or exists (select 1 from gah_private.entitlements e
                                 where e.user_id = auth.uid() and e.revoked_at is null
                                   and e.valid_until > statement_timestamp()));
$$;
revoke all on function gah_private.can_access(uuid) from public, anon;
grant execute on function gah_private.can_access(uuid) to authenticated;

create table if not exists public.nodes (
    workspace_id uuid not null references public.workspaces on delete cascade,
    node_id text not null check (length(node_id) between 1 and 128),
    owner_id uuid not null references auth.users on delete cascade,
    display_name text not null check (length(display_name) between 1 and 100),
    advertised_url text not null check (length(advertised_url) <= 2048 and advertised_url ~ '^https?://'),
    version text not null check (length(version) between 1 and 64),
    schema_digest text not null check (length(schema_digest) between 1 and 128),
    transport_mode text not null check (transport_mode in ('trusted_lan', 'authenticated_remote')),
    profiles text[] not null default '{}' check (cardinality(profiles) <= 100 and octet_length(profiles::text) <= 16000),
    sharing_scope text not null default 'private' check (sharing_scope in ('private', 'workspace', 'selected')),
    registered_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now(),
    revoked_at timestamptz,
    primary key (workspace_id, node_id)
);
create table if not exists gah_private.node_grants (
    workspace_id uuid not null,
    node_id text not null,
    user_id uuid not null,
    permission text not null check (permission in ('use', 'manage')),
    primary key (workspace_id, node_id, user_id),
    foreign key (workspace_id, node_id) references public.nodes on delete cascade,
    foreign key (workspace_id, user_id) references gah_private.memberships on delete cascade
);
alter table gah_private.node_grants enable row level security;
revoke all on gah_private.node_grants from public, anon, authenticated;
grant all on gah_private.node_grants to service_role;

create table if not exists public.work_items (
    workspace_id uuid not null references public.workspaces on delete cascade,
    work_id text not null check (length(work_id) between 1 and 256),
    node_id text,
    profile text not null check (length(profile) between 1 and 128),
    state text not null check (state in ('queued', 'running', 'completed', 'failed', 'cancelled')),
    updated_at timestamptz not null default now(),
    primary key (workspace_id, work_id),
    foreign key (workspace_id, node_id) references public.nodes (workspace_id, node_id)
);
create table if not exists public.events (
    workspace_id uuid not null references public.workspaces on delete cascade,
    event_id text not null check (length(event_id) between 1 and 256),
    work_id text,
    kind text not null check (length(kind) between 1 and 128),
    occurred_at timestamptz not null default now(),
    primary key (workspace_id, event_id),
    foreign key (workspace_id, work_id) references public.work_items (workspace_id, work_id)
);

alter table public.workspaces enable row level security;
alter table public.nodes enable row level security;
alter table public.work_items enable row level security;
alter table public.events enable row level security;
revoke all on public.workspaces, public.nodes, public.work_items, public.events from public, anon, authenticated;
grant select on public.workspaces, public.nodes, public.work_items, public.events to authenticated;
grant all on public.workspaces, public.nodes, public.work_items, public.events to service_role;

drop policy if exists cloud_workspace_read on public.workspaces;
create policy cloud_workspace_read on public.workspaces for select to authenticated
    using (gah_private.can_access(id));
drop policy if exists cloud_nodes_read on public.nodes;
-- A resource owner's entitlement also gates visibility. Revoked owners cannot
-- keep offering resources through someone else's still-valid membership.
create or replace function gah_private.owner_active(p_owner_id uuid, p_workspace_id uuid)
returns boolean language sql stable security definer set search_path = '' as $$
    select exists (select 1 from gah_private.memberships m
                   where m.workspace_id = p_workspace_id and m.user_id = p_owner_id)
       and exists (select 1 from gah_private.deployment d
                   where not d.requires_entitlement
                      or exists (select 1 from gah_private.entitlements e
                                 where e.user_id = p_owner_id and e.revoked_at is null
                                   and e.valid_until > statement_timestamp()));
$$;
revoke all on function gah_private.owner_active(uuid, uuid) from public, anon;
grant execute on function gah_private.owner_active(uuid, uuid) to authenticated;
-- Permission is rechecked against live memberships and entitlements on every call.
create or replace function gah_private.node_permission(p_workspace_id uuid, p_node_id text)
returns text language sql stable security definer set search_path = '' as $$
    select case when n.owner_id = auth.uid() then 'owner'
                when n.sharing_scope = 'selected' then g.permission
                when n.sharing_scope = 'workspace' then 'use' end
      from public.nodes n
      left join gah_private.node_grants g on g.workspace_id = n.workspace_id
           and g.node_id = n.node_id and g.user_id = auth.uid()
     where n.workspace_id = p_workspace_id and n.node_id = p_node_id
       and gah_private.can_access(p_workspace_id)
       and gah_private.owner_active(n.owner_id, p_workspace_id)
       and (n.owner_id = auth.uid() or n.revoked_at is null);
$$;
revoke all on function gah_private.node_permission(uuid, text) from public, anon;
grant execute on function gah_private.node_permission(uuid, text) to authenticated;
create policy cloud_nodes_read on public.nodes for select to authenticated
    using (gah_private.node_permission(workspace_id, node_id) is not null);
drop policy if exists cloud_work_read on public.work_items;
create policy cloud_work_read on public.work_items for select to authenticated
    using (gah_private.can_access(workspace_id));
drop policy if exists cloud_events_read on public.events;
create policy cloud_events_read on public.events for select to authenticated
    using (gah_private.can_access(workspace_id));

-- Only these RPCs accept untrusted writes. No arbitrary JSON, credentials,
-- owner IDs, plan fields, or client timestamps can enter the directory.
create or replace function public.register_cloud_node(
    p_workspace_id uuid, p_node_id text, p_display_name text,
    p_advertised_url text, p_version text, p_schema_digest text,
    p_transport_mode text, p_profiles text[]
) returns void language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then
        raise exception 'cloud_access_denied' using errcode = '42501';
    end if;
    if p_advertised_url !~ '^https?://[^/?#@[:space:]]+(:[0-9]+)?/?$' then
        raise exception 'invalid_node_origin' using errcode = '22023';
    end if;
    insert into public.nodes (workspace_id, node_id, owner_id, display_name,
                             advertised_url, version, schema_digest, transport_mode, profiles)
    values (p_workspace_id, p_node_id, auth.uid(), p_display_name,
            p_advertised_url, p_version, p_schema_digest, p_transport_mode, p_profiles)
    on conflict (workspace_id, node_id) do update
        set display_name = excluded.display_name, advertised_url = excluded.advertised_url,
            version = excluded.version, schema_digest = excluded.schema_digest,
            transport_mode = excluded.transport_mode, profiles = excluded.profiles,
            last_seen_at = statement_timestamp()
        where nodes.owner_id = auth.uid() and nodes.revoked_at is null;
    if not found then
        raise exception 'node_write_denied' using errcode = '42501';
    end if;
end;
$$;

create or replace function public.manage_cloud_node(
    p_workspace_id uuid, p_node_id text, p_action text
) returns void language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then
        raise exception 'cloud_access_denied' using errcode = '42501';
    end if;
    if p_action not in ('private', 'workspace', 'selected', 'revoke') or p_action is null then
        raise exception 'invalid_node_action' using errcode = '22023';
    end if;
    update public.nodes
       set sharing_scope = case when p_action = 'revoke' then 'private' else p_action end,
           revoked_at = case when p_action = 'revoke' then statement_timestamp() else revoked_at end
     where workspace_id = p_workspace_id and node_id = p_node_id
       and gah_private.node_permission(p_workspace_id, p_node_id) in ('owner', 'manage')
       and revoked_at is null;
    if not found then
        raise exception 'node_write_denied' using errcode = '42501';
    end if;
end;
$$;
-- Only the resource owner can delegate; managers cannot grant themselves use.
create or replace function public.set_cloud_node_grant(
    p_workspace_id uuid, p_node_id text, p_user_id uuid, p_permission text
) returns void language plpgsql security definer set search_path = '' as $$
begin
    if gah_private.node_permission(p_workspace_id, p_node_id) is distinct from 'owner' then
        raise exception 'node_write_denied' using errcode = '42501';
    end if;
    if p_permission is null or p_permission not in ('use', 'manage', 'remove') then
        raise exception 'invalid_node_permission' using errcode = '22023';
    end if;
    if not exists (select 1 from gah_private.memberships
                   where workspace_id = p_workspace_id and user_id = p_user_id) then
        raise exception 'workspace_member_required' using errcode = '42501';
    end if;
    if p_permission = 'remove' then
        delete from gah_private.node_grants where workspace_id = p_workspace_id
            and node_id = p_node_id and user_id = p_user_id;
    else
        insert into gah_private.node_grants values (p_workspace_id, p_node_id, p_user_id, p_permission)
        on conflict (workspace_id, node_id, user_id) do update set permission = excluded.permission;
    end if;
end;
$$;

create or replace function public.cloud_workspace_members(p_workspace_id uuid)
returns table (user_id uuid, display_name text)
language sql stable security definer set search_path = '' as $$
    select m.user_id, m.display_name from gah_private.memberships m
     where m.workspace_id = p_workspace_id and gah_private.can_access(p_workspace_id);
$$;

create or replace function public.cloud_node_permissions(p_workspace_id uuid)
returns table (node_id text, permission text)
language sql stable security definer set search_path = '' as $$
    select n.node_id, gah_private.node_permission(n.workspace_id, n.node_id)
      from public.nodes n where n.workspace_id = p_workspace_id
       and gah_private.node_permission(n.workspace_id, n.node_id) is not null;
$$;

create or replace function public.cloud_node_grants(p_workspace_id uuid, p_node_id text)
returns table (user_id uuid, permission text)
language sql stable security definer set search_path = '' as $$
    select g.user_id, g.permission from gah_private.node_grants g
     where g.workspace_id = p_workspace_id and g.node_id = p_node_id
       and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
$$;
revoke all on function public.set_cloud_node_grant(uuid, text, uuid, text),
    public.cloud_workspace_members(uuid), public.cloud_node_permissions(uuid),
    public.cloud_node_grants(uuid, text) from public, anon, service_role;
grant execute on function public.set_cloud_node_grant(uuid, text, uuid, text),
    public.cloud_workspace_members(uuid), public.cloud_node_permissions(uuid),
    public.cloud_node_grants(uuid, text) to authenticated;
revoke all on function public.register_cloud_node(uuid, text, text, text, text, text, text, text[]) from public, anon, service_role;
revoke all on function public.manage_cloud_node(uuid, text, text) from public, anon, service_role;
grant execute on function public.register_cloud_node(uuid, text, text, text, text, text, text, text[]) to authenticated;
grant execute on function public.manage_cloud_node(uuid, text, text) to authenticated;
commit;
