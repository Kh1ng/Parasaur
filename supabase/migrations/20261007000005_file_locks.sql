-- ORG-06: exclusive file ownership, granted by the server to a running, owned run.
-- Scope: a lock covers (repository, scope, path). The caller names the scope; GAH callers should pass the integration
-- target branch, so the same file on unrelated branches is independent. Paths are case-insensitive and NFC-normalized.
-- A lock lives until it is resolved, never by lease expiry: a disconnected process may still be writing, so a lost run's
-- locks stay blocking until the holder releases them, a handoff or takeover transfers them, or an authorized person
-- resolves them with a recorded disposition. Acquisition is all-or-nothing and never waits, so it cannot deadlock.
-- This is authority, not enforcement: it only binds worker environments that call check_cloud_file_access first.
begin;

create table if not exists public.file_locks (
    workspace_id uuid not null,
    repo_id uuid not null,
    scope text not null check (length(scope) between 1 and 255),
    path_key text not null check (length(path_key) <= 1024),
    path text not null check (length(path) <= 1024),
    work_id text not null,
    task_id uuid not null,
    owner_id uuid not null,
    generation bigint not null,
    acquired_at timestamptz not null default now(),
    resolved_at timestamptz,
    resolution text check (resolution in ('released', 'handed_off', 'preserved', 'discarded')),
    primary key (workspace_id, work_id, repo_id, scope, path_key),
    foreign key (workspace_id, work_id) references public.work_items on delete cascade,
    foreign key (workspace_id, repo_id) references public.repositories on delete cascade
);
-- path_key '' is the explicit whole-repository lock for a run whose edit scope is unknown.
create unique index if not exists file_lock_active on public.file_locks (workspace_id, repo_id, scope, path_key) where resolved_at is null;
alter table public.file_locks enable row level security;
revoke all on public.file_locks from public, anon, authenticated;
grant select on public.file_locks to authenticated;
grant all on public.file_locks to service_role;
-- ponytail: every member sees every lock, like tasks, until ORG-10 settles visibility.
drop policy if exists file_locks_read on public.file_locks;
create policy file_locks_read on public.file_locks for select to authenticated using (gah_private.can_access(workspace_id));

-- Repository-relative, forward-slash, no ".", "..", empty segments, drive letters, or trailing slash. The caller resolves
-- symlinks to real repository paths first; the server cannot see the filesystem.
create or replace function gah_private.canon_path(p text) returns text
language plpgsql immutable set search_path = '' as $$
declare n text := normalize(p, nfc);
begin
    if n is null or n = '' or length(n) > 1024 or n ~ '\\' or n ~ '^[A-Za-z]:' or n ~ '^/' or n ~ '/$'
        or n ~ '(^|/)(\.\.?)?(/|$)' then
        raise exception 'invalid_path' using errcode = '22023'; end if;
    return n;
end;
$$;

-- The run must be the live holder of its task. Returns the run with its repository.
create or replace function gah_private.lock_run(p_device_token text, p_work_id text, p_claim_id uuid, out w public.work_items, out repo uuid)
language plpgsql stable security definer set search_path = '' as $$
begin
    if not (public.check_cloud_ownership(p_device_token, p_work_id, p_claim_id)->>'owned')::boolean then
        raise exception 'ownership_lost' using errcode = '42501'; end if;
    select * into w from public.work_items where work_id = p_work_id and claim_id = p_claim_id;
    select repo_id into repo from public.tasks where workspace_id = w.workspace_id and task_id = w.task_id;
    if repo is null then raise exception 'task_required' using errcode = '22023'; end if;
end;
$$;
revoke all on function gah_private.lock_run(text, text, uuid) from public, anon, authenticated;

-- Acquire a path set, or a whole-repository lock when the edit scope is unknown. Denial is a normal result that names
-- each blocking holder and task; nothing is granted unless every path is.
create or replace function public.acquire_cloud_file_locks(p_device_token text, p_work_id text, p_claim_id uuid, p_scope text,
    p_paths text[], p_whole_repository boolean default false)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare h record; w public.work_items; r uuid; keys text[]; blocked jsonb; n integer;
begin
    select * into h from gah_private.lock_run(p_device_token, p_work_id, p_claim_id); w := h.w; r := h.repo;
    if p_scope is null or length(p_scope) not between 1 and 255 or (p_whole_repository is not true and coalesce(cardinality(p_paths), 0) not between 1 and 200)
        or (p_whole_repository is true and p_paths is not null) then raise exception 'invalid_lock_request' using errcode = '22023'; end if;
    -- ponytail: lower() follows the database collation; pin it if mixed-locale fleets disagree on case folding.
    select coalesce(array_agg(lower(gah_private.canon_path(x))), '{}') into keys from unnest(p_paths) x;
    if (select count(*) from public.file_locks where workspace_id = w.workspace_id and work_id = w.work_id and resolved_at is null) + cardinality(keys) > 2000 then
        raise exception 'lock_limit' using errcode = '54000'; end if;
    perform pg_advisory_xact_lock(hashtextextended(r::text || ':' || p_scope, 0));
    select coalesce(jsonb_agg(jsonb_build_object('path', l.path, 'whole_repository', l.path_key = '', 'work_id', l.work_id, 'task_id', l.task_id,
            'holder', (select m.display_name from gah_private.memberships m where m.workspace_id = l.workspace_id and m.user_id = l.owner_id))), '[]') into blocked
        from public.file_locks l where l.workspace_id = w.workspace_id and l.repo_id = r and l.scope = p_scope and l.resolved_at is null
            and l.work_id <> w.work_id and (p_whole_repository is true or l.path_key = '' or l.path_key = any(keys));
    if jsonb_array_length(blocked) > 0 then return jsonb_build_object('acquired', false, 'blocked', blocked); end if;
    -- A path this run released earlier keeps its row, so acquiring it again reopens that row; a path it holds is unchanged.
    insert into public.file_locks as l (workspace_id, repo_id, scope, path_key, path, work_id, task_id, owner_id, generation)
    select distinct on (k) w.workspace_id, r, p_scope, k, p, w.work_id, w.task_id, w.requester_id, w.task_generation
    from unnest(case when p_whole_repository is true then array[null::text] else p_paths end) x,
        lateral (select case when p_whole_repository is true then '' else lower(gah_private.canon_path(x)) end k,
            case when p_whole_repository is true then '(whole repository)' else gah_private.canon_path(x) end p) c
    order by k
    on conflict (workspace_id, work_id, repo_id, scope, path_key) do update
        set path = excluded.path, task_id = excluded.task_id, owner_id = excluded.owner_id, generation = excluded.generation,
            acquired_at = statement_timestamp(), resolved_at = null, resolution = null
        where l.resolved_at is not null;
    get diagnostics n = row_count;
    if n > 0 then
        insert into public.events (workspace_id, event_id, work_id, kind, actor_id, device_id, agent_session_id, payload)
        values (w.workspace_id, gen_random_uuid()::text, w.work_id, 'file.locked', w.requester_id, w.device_id, w.agent_session_id,
            jsonb_build_object('scope', p_scope, 'count', n, 'whole_repository', p_whole_repository is true));
    end if;
    return jsonb_build_object('acquired', true, 'count', n);
end;
$$;

-- Call before a mutation: allowed only when this run is still the owner and holds every path (or the whole repository).
create or replace function public.check_cloud_file_access(p_device_token text, p_work_id text, p_claim_id uuid, p_scope text, p_paths text[])
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare h record; w public.work_items; r uuid; denied jsonb;
begin
    if not (public.check_cloud_ownership(p_device_token, p_work_id, p_claim_id)->>'owned')::boolean then
        return jsonb_build_object('allowed', false, 'reason', 'ownership_lost', 'denied', to_jsonb(p_paths)); end if;
    select * into h from gah_private.lock_run(p_device_token, p_work_id, p_claim_id); w := h.w; r := h.repo;
    select coalesce(jsonb_agg(x), '[]') into denied from unnest(p_paths) x where not exists (
        select 1 from public.file_locks l where l.workspace_id = w.workspace_id and l.repo_id = r and l.scope = p_scope and l.work_id = w.work_id
            and l.resolved_at is null and l.path_key in ('', lower(gah_private.canon_path(x))));
    return jsonb_build_object('allowed', jsonb_array_length(denied) = 0, 'reason', case when jsonb_array_length(denied) > 0 then 'not_locked' end, 'denied', denied);
end;
$$;

create or replace function public.release_cloud_file_locks(p_device_token text, p_work_id text, p_claim_id uuid, p_scope text, p_paths text[])
returns integer language plpgsql security definer set search_path = '' as $$
declare h record; n integer;
begin
    select * into h from gah_private.lock_run(p_device_token, p_work_id, p_claim_id);
    update public.file_locks l set resolved_at = statement_timestamp(), resolution = 'released'
    where l.workspace_id = (h.w).workspace_id and l.work_id = p_work_id and l.scope = p_scope and l.resolved_at is null
        and l.path_key in (select lower(gah_private.canon_path(x)) from unnest(p_paths) x);
    get diagnostics n = row_count;
    return n;
end;
$$;

-- An authorized person closes the locks of a run that ended without releasing them. A run whose lease is still live
-- cannot be resolved: stop it with a takeover first, because it may still be writing.
create or replace function public.resolve_cloud_file_locks(p_workspace_id uuid, p_work_id text, p_disposition text)
returns integer language plpgsql security definer set search_path = '' as $$
declare w public.work_items; n integer;
begin
    if p_disposition is null or p_disposition not in ('preserved', 'discarded', 'handed_off') then raise exception 'invalid_disposition' using errcode = '22023'; end if;
    select * into w from public.work_items where workspace_id = p_workspace_id and work_id = p_work_id for update;
    if not found or not gah_private.can_access(p_workspace_id) or not (w.requester_id = auth.uid() or gah_private.workspace_owner(p_workspace_id)
        or gah_private.node_permission(p_workspace_id, w.node_id) in ('owner', 'manage')) then raise exception 'resolve_denied' using errcode = '42501'; end if;
    if w.state = 'queued' or (w.state = 'running' and w.leased_until > statement_timestamp()) then raise exception 'run_still_active' using errcode = '55006'; end if;
    update public.file_locks set resolved_at = statement_timestamp(), resolution = p_disposition
        where workspace_id = p_workspace_id and work_id = p_work_id and resolved_at is null;
    get diagnostics n = row_count;
    insert into public.events (workspace_id, event_id, work_id, kind, actor_id, payload)
    values (p_workspace_id, gen_random_uuid()::text, p_work_id, 'file.resolved', auth.uid(), jsonb_build_object('disposition', p_disposition, 'count', n));
    return n;
end;
$$;

-- A holder that finishes has acknowledged its end, so its locks are released.
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
    update public.file_locks set resolved_at = statement_timestamp(), resolution = 'released'
        where workspace_id = w.workspace_id and work_id = w.work_id and resolved_at is null;
    perform gah_private.audit(w, 'cloud.' || w.state); return true;
end;
$$;

-- Handoff and takeover are explicit authorized decisions, so they transfer the stopped run's locks.
create or replace function gah_private.stop_run(w public.work_items)
returns void language plpgsql security definer set search_path = '' as $$
begin
    -- As in ORG-05, w may be stale: a claim that landed after the read is told to stop, never cancelled.
    if w.state = 'queued' then
        update public.work_items set state = 'cancelled', updated_at = statement_timestamp()
            where workspace_id = w.workspace_id and work_id = w.work_id and state = 'queued';
        if found then perform gah_private.audit(w, 'cloud.handoff_stopped'); end if;
    end if;
    if w.state <> 'queued' or not found then
        update public.work_items set stop_requested_at = statement_timestamp()
            where workspace_id = w.workspace_id and work_id = w.work_id and state = 'running';
    end if;
    update public.file_locks set resolved_at = statement_timestamp(), resolution = 'handed_off'
        where workspace_id = w.workspace_id and work_id = w.work_id and resolved_at is null;
end;
$$;
revoke all on function gah_private.stop_run(public.work_items) from public, anon, authenticated;

revoke all on function public.acquire_cloud_file_locks(text, text, uuid, text, text[], boolean), public.check_cloud_file_access(text, text, uuid, text, text[]),
    public.release_cloud_file_locks(text, text, uuid, text, text[]) from public, anon, authenticated, service_role;
grant execute on function public.acquire_cloud_file_locks(text, text, uuid, text, text[], boolean), public.check_cloud_file_access(text, text, uuid, text, text[]),
    public.release_cloud_file_locks(text, text, uuid, text, text[]) to anon, authenticated;
revoke all on function public.resolve_cloud_file_locks(uuid, text, text) from public, anon;
grant execute on function public.resolve_cloud_file_locks(uuid, text, text) to authenticated;

commit;
