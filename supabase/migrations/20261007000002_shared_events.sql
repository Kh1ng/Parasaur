-- ORG-03: durable, bounded, resumable work events.
-- Event identity: server events keep random ids. A device event's id is "<agent_session_id>:<run_seq>", where run_seq is
-- the sender's 1-based counter for that attempt, so redelivery is the same event. work_id survives a requeue, but every
-- claim is a new agent session, so a later attempt restarting at run_seq 1 never collides with an earlier one. Order within a run is run_seq; order across
-- a workspace is seq, a gapless counter assigned under a per-workspace row lock (commit order, no skipped rows).
begin;

alter table public.events add column if not exists seq bigint;
alter table public.events add column if not exists run_seq integer check (run_seq between 1 and 10000);
alter table public.events add column if not exists schema_version integer not null default 1;
alter table public.events add column if not exists payload jsonb not null default '{}'
    check (jsonb_typeof(payload) = 'object' and octet_length(payload::text) <= 4096);

create table if not exists gah_private.event_cursors (
    workspace_id uuid primary key references public.workspaces on delete cascade,
    last bigint not null default 0
);
alter table gah_private.event_cursors enable row level security;
revoke all on gah_private.event_cursors from public, anon, authenticated;
grant all on gah_private.event_cursors to service_role;

-- Backfill events written before this migration, then keep the cursor ahead of them.
with ranked as (select workspace_id, event_id,
        coalesce((select max(seq) from public.events m where m.workspace_id = e.workspace_id), 0)
        + row_number() over (partition by workspace_id order by occurred_at, event_id) as n
    from public.events e where seq is null)
update public.events e set seq = ranked.n from ranked where e.workspace_id = ranked.workspace_id and e.event_id = ranked.event_id;
insert into gah_private.event_cursors (workspace_id, last) select workspace_id, max(seq) from public.events group by workspace_id
    on conflict (workspace_id) do update set last = greatest(gah_private.event_cursors.last, excluded.last);

create or replace function gah_private.assign_event_seq() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
    insert into gah_private.event_cursors as c (workspace_id, last) values (new.workspace_id, 1)
        on conflict (workspace_id) do update set last = c.last + 1 returning c.last into new.seq;
    return new;
end;
$$;
revoke all on function gah_private.assign_event_seq() from public, anon, authenticated;
drop trigger if exists events_assign_seq on public.events;
create trigger events_assign_seq before insert on public.events for each row when (new.seq is null) execute function gah_private.assign_event_seq();

alter table public.events alter column seq set not null;
create unique index if not exists events_seq on public.events (workspace_id, seq);
drop index if exists public.events_run_seq;
create unique index if not exists events_session_run_seq on public.events (workspace_id, agent_session_id, run_seq) where run_seq is not null;

-- A device reports run facts for the run it currently holds. Attribution is copied from the work row, so it is never
-- caller input, and payloads are limited to named scalar fields so no source, prompt, or transcript can ride along.
-- Unknown cost is the absence of cost_micros, never zero.
create or replace function public.report_cloud_event(p_device_token text, p_work_id text, p_claim_id uuid, p_run_seq integer, p_kind text, p_payload jsonb)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; prior public.events; e public.events; k text; v jsonb;
begin
    if p_kind is null or p_kind not in ('run.state', 'run.handoff', 'run.outcome', 'run.usage')
        or p_run_seq is null or p_run_seq not between 1 and 10000 or jsonb_typeof(p_payload) is distinct from 'object' then
        raise exception 'invalid_event' using errcode = '22023'; end if;
    for k, v in select * from jsonb_each(p_payload) loop
        if k <> all (array['state', 'reason', 'exit_code', 'model', 'provider', 'target', 'input_tokens', 'output_tokens', 'cost_micros', 'cost_estimated'])
            or jsonb_typeof(v) not in ('string', 'number', 'boolean', 'null') or length(v::text) > 202 then
            raise exception 'invalid_event_payload' using errcode = '22023'; end if;
    end loop;
    select * into w from public.work_items where workspace_id = d.workspace_id and work_id = p_work_id and node_id = d.node_id
        and device_id = d.id and claim_id = p_claim_id and state = 'running' and leased_until > statement_timestamp() for update;
    if not found or not gah_private.job_allowed(w) then raise exception 'event_access_denied' using errcode = '42501'; end if;
    select * into prior from public.events where workspace_id = w.workspace_id and agent_session_id = w.agent_session_id and run_seq = p_run_seq;
    if found then
        if (prior.kind, prior.payload) is distinct from (p_kind, p_payload) then raise exception 'event_conflict' using errcode = '22023'; end if;
        return jsonb_build_object('seq', prior.seq, 'duplicate', true);
    end if;
    insert into public.events (workspace_id, event_id, work_id, kind, actor_id, device_id, agent_session_id, run_seq, payload)
    values (w.workspace_id, w.agent_session_id || ':' || p_run_seq, w.work_id, p_kind, w.requester_id, w.device_id, w.agent_session_id, p_run_seq, p_payload)
    returning * into e;
    return jsonb_build_object('seq', e.seq, 'duplicate', false);
end;
$$;

-- The oldest stored seq of a workspace, whatever the caller may see. A reader who cannot see the first events must not
-- mistake them for pruned history, so the low-water mark is computed past row security and returns only a number.
create or replace function gah_private.event_low_water(p_workspace_id uuid)
returns bigint language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    return (select min(seq) from public.events where workspace_id = p_workspace_id);
end;
$$;
revoke all on function gah_private.event_low_water(uuid) from public, anon;
grant execute on function gah_private.event_low_water(uuid) to authenticated;

-- Resume from a cursor. gap is true when events after the cursor are no longer stored; the reader must then resync.
create or replace function public.cloud_events_since(p_workspace_id uuid, p_after bigint default 0, p_limit integer default 200)
returns jsonb language plpgsql stable security invoker set search_path = '' as $$
declare rows jsonb; oldest bigint;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    oldest := gah_private.event_low_water(p_workspace_id);
    select coalesce(jsonb_agg(to_jsonb(x) order by x.seq), '[]') into rows from (
        select seq, event_id, work_id, kind, occurred_at, actor_id, device_id, agent_session_id, run_seq, schema_version, payload
        from public.events where workspace_id = p_workspace_id and seq > greatest(p_after, 0) order by seq limit least(greatest(p_limit, 1), 500)) x;
    return jsonb_build_object('events', rows, 'gap', oldest is not null and greatest(p_after, 0) + 1 < oldest,
        'next_cursor', coalesce((select max((e->>'seq')::bigint) from jsonb_array_elements(rows) e), greatest(p_after, 0)));
end;
$$;

-- last_observed_* is what the sender last reported. confirmed_state is the server's own record; lease_live says
-- whether anything still holds the run. missing_run_seqs lists reports not yet received. Device reports are read for the
-- current attempt (agent session) only, so a requeued run never mixes its attempts; a queued run has none.
create or replace function public.cloud_run_status(p_workspace_id uuid, p_work_id text)
returns jsonb language plpgsql stable security invoker set search_path = '' as $$
declare w public.work_items; last_state public.events; top integer;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    select * into w from public.work_items where workspace_id = p_workspace_id and work_id = p_work_id;
    if not found then raise exception 'work_not_found' using errcode = '22023'; end if;
    select * into last_state from public.events where workspace_id = p_workspace_id and work_id = p_work_id
        and agent_session_id = w.agent_session_id and kind = 'run.state' order by run_seq desc limit 1;
    select max(run_seq) into top from public.events where workspace_id = p_workspace_id and work_id = p_work_id
        and agent_session_id = w.agent_session_id;
    return jsonb_build_object('agent_session_id', w.agent_session_id, 'confirmed_state', w.state, 'lease_live', w.state = 'running' and w.leased_until > statement_timestamp(),
        'last_observed_state', last_state.payload->>'state', 'last_observed_at', last_state.occurred_at, 'last_run_seq', top,
        'missing_run_seqs', coalesce((select jsonb_agg(s) from generate_series(1, coalesce(top, 0)) s
            where not exists (select 1 from public.events e where e.workspace_id = p_workspace_id and e.work_id = p_work_id
                and e.agent_session_id = w.agent_session_id and e.run_seq = s)), '[]'));
end;
$$;

revoke all on function public.report_cloud_event(text, text, uuid, integer, text, jsonb) from public, anon, authenticated, service_role;
grant execute on function public.report_cloud_event(text, text, uuid, integer, text, jsonb) to anon, authenticated;
revoke all on function public.cloud_events_since(uuid, bigint, integer), public.cloud_run_status(uuid, text) from public, anon;
grant execute on function public.cloud_events_since(uuid, bigint, integer), public.cloud_run_status(uuid, text) to authenticated;

commit;
