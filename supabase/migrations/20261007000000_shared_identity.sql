-- ORG-01: one identity for a repository, a task, and (via work_items.work_id) each execution attempt.
-- The workspace is the organization boundary. Profile names and local paths only map onto these ids.
begin;

create table if not exists public.repositories (
    workspace_id uuid not null references public.workspaces on delete cascade,
    repo_id uuid not null default gen_random_uuid(),
    source text not null check (source ~ '^[a-z0-9][a-z0-9.-]{0,252}$'),
    path text not null check (path ~ '^([a-z0-9._-]*[a-z0-9_-][a-z0-9._-]*)(/[a-z0-9._-]*[a-z0-9_-][a-z0-9._-]*)+$' and length(path) <= 255),
    primary key (workspace_id, repo_id),
    unique (workspace_id, source, path)
);

-- A node profile is a local alias. Several aliases, on any node, may name one repository.
create table if not exists public.repository_aliases (
    workspace_id uuid not null,
    node_id text not null,
    profile text not null check (length(profile) between 1 and 128),
    repo_id uuid not null,
    local_path text check (length(local_path) <= 1024),
    primary key (workspace_id, node_id, profile),
    foreign key (workspace_id, node_id) references public.nodes on delete cascade,
    foreign key (workspace_id, repo_id) references public.repositories on delete cascade
);

-- kind 'issue' and 'review' use the tracker number; 'manual' uses an explicit key, matched exactly.
create table if not exists public.tasks (
    workspace_id uuid not null,
    task_id uuid not null default gen_random_uuid(),
    repo_id uuid not null,
    kind text not null check (kind in ('issue', 'review', 'manual')),
    ref text not null check (case kind when 'manual' then ref ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$'
                                       else ref ~ '^[1-9][0-9]{0,9}$' end),
    primary key (workspace_id, task_id),
    unique (workspace_id, repo_id, kind, ref),
    foreign key (workspace_id, repo_id) references public.repositories on delete cascade
);

alter table public.repositories enable row level security;
alter table public.repository_aliases enable row level security;
alter table public.tasks enable row level security;
revoke all on public.repositories, public.repository_aliases, public.tasks from public, anon, authenticated;
grant select on public.repositories, public.repository_aliases, public.tasks to authenticated;
grant all on public.repositories, public.repository_aliases, public.tasks to service_role;
drop policy if exists repositories_read on public.repositories;
create policy repositories_read on public.repositories for select to authenticated using (gah_private.can_access(workspace_id));
-- Local paths describe a machine, so only that node's owner reads its aliases.
drop policy if exists repository_aliases_read on public.repository_aliases;
create policy repository_aliases_read on public.repository_aliases for select to authenticated
    using (gah_private.node_permission(workspace_id, node_id) = 'owner');
-- ponytail: every member sees every task until ORG-10 settles visibility.
drop policy if exists tasks_read on public.tasks;
create policy tasks_read on public.tasks for select to authenticated using (gah_private.can_access(workspace_id));

-- Workspace owners register a repository once. Source and path are folded to lowercase; a trailing ".git" is dropped.
create or replace function public.register_cloud_repository(p_workspace_id uuid, p_source text, p_path text)
returns uuid language plpgsql security definer set search_path = '' as $$
declare s text := lower(btrim(p_source)); p text := regexp_replace(lower(btrim(p_path)), '\.git$', ''); r uuid;
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    if s !~ '^[a-z0-9][a-z0-9.-]{0,252}$' or p !~ '^([a-z0-9._-]*[a-z0-9_-][a-z0-9._-]*)(/[a-z0-9._-]*[a-z0-9_-][a-z0-9._-]*)+$' or length(p) > 255 then
        raise exception 'invalid_repository' using errcode = '22023'; end if;
    insert into public.repositories (workspace_id, source, path) values (p_workspace_id, s, p) on conflict (workspace_id, source, path) do nothing;
    select repo_id into r from public.repositories where workspace_id = p_workspace_id and source = s and path = p;
    return r;
end;
$$;

-- The owner of a node maps that node's own profile onto a registered repository.
create or replace function public.map_cloud_repository_alias(p_workspace_id uuid, p_node_id text, p_profile text, p_repo_id uuid, p_local_path text default null)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if gah_private.node_permission(p_workspace_id, p_node_id) is distinct from 'owner' then raise exception 'node_write_denied' using errcode = '42501'; end if;
    if not exists (select 1 from public.repositories where workspace_id = p_workspace_id and repo_id = p_repo_id) then
        raise exception 'repository_not_found' using errcode = '22023'; end if;
    insert into public.repository_aliases (workspace_id, node_id, profile, repo_id, local_path)
    values (p_workspace_id, p_node_id, p_profile, p_repo_id, p_local_path)
    on conflict (workspace_id, node_id, profile) do update set repo_id = excluded.repo_id, local_path = excluded.local_path;
end;
$$;

-- Resolve a task through a node profile alias, creating it on first use. Free text is never matched, only an exact ref.
-- Two requests for one (repository, kind, ref) return one task_id whatever profile or node named it.
create or replace function public.resolve_cloud_task(p_workspace_id uuid, p_node_id text, p_profile text, p_kind text, p_ref text)
returns uuid language plpgsql security definer set search_path = '' as $$
declare r uuid; t uuid;
begin
    if gah_private.node_permission(p_workspace_id, p_node_id) is null then raise exception 'node_access_denied' using errcode = '42501'; end if;
    select repo_id into r from public.repository_aliases where workspace_id = p_workspace_id and node_id = p_node_id and profile = p_profile;
    if r is null then raise exception 'repository_unmapped' using errcode = '22023'; end if;
    -- ponytail: no per-workspace task cap; add one if members can mint tasks abusively.
    begin
        insert into public.tasks (workspace_id, repo_id, kind, ref) values (p_workspace_id, r, p_kind, p_ref)
        on conflict (workspace_id, repo_id, kind, ref) do nothing;
    exception when check_violation then raise exception 'invalid_task' using errcode = '22023'; end;
    select task_id into t from public.tasks where workspace_id = p_workspace_id and repo_id = r and kind = p_kind and ref = p_ref;
    return t;
end;
$$;

revoke all on function public.register_cloud_repository(uuid, text, text), public.map_cloud_repository_alias(uuid, text, text, uuid, text),
    public.resolve_cloud_task(uuid, text, text, text, text) from public, anon;
grant execute on function public.register_cloud_repository(uuid, text, text), public.map_cloud_repository_alias(uuid, text, text, uuid, text),
    public.resolve_cloud_task(uuid, text, text, text, text) to authenticated;

commit;
