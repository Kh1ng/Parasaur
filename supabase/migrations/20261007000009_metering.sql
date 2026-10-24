-- ORG-14: admission and limits for managed (paid) compute. Execution accounting only: pricing, payment collection, and
-- invoicing are a separate product decision, so nothing here names a billing provider or a price.
-- Scope: only runs placed on nodes with worker_kind 'managed' are metered. A workspace has no managed access until an
--   owner enables it with explicit limits, so a missing or unknown configuration or entitlement never authorizes paid work.
-- Admission: an AFTER INSERT trigger on work_items under a lock on the limits row, so simultaneous launches are serialized
--   and the enqueue rolls back (including its task claim) on denial. It checks the launcher, concurrency, and budget.
-- Reservation: each admitted run reserves run_reservation_micros while it is live (queued, or running with a live lease).
--   Reservations are derived from run state, never stored as a balance, so cancellation, completion, and a crashed run's
--   lease lapse release them without cleanup, and a retry is a new run with its own reservation and its own cost events.
-- Spend, for the calendar month (UTC) the run was admitted in: reported cost plus estimated cost (kept separate), plus the
--   unspent part of each live run's reservation, plus the whole reservation of a started run that has no cost report yet.
--   Unknown cost is never zero; an owner can record a measured cost to reconcile a crashed run.
-- Overrun: delayed usage can push spend past the budget. Running work is not killed for that; new admission stops and
--   the allowance shows a negative remainder. Each run stops at the duration limit that applied when it was admitted.
-- Compute is metered in seconds, separate from model cost, until a rate is decided.
begin;

create table if not exists gah_private.cloud_limits (
    workspace_id uuid primary key references public.workspaces on delete cascade,
    managed_enabled boolean not null default false,
    managed_max_concurrent integer check (managed_max_concurrent between 1 and 1000),
    managed_max_duration_seconds integer check (managed_max_duration_seconds between 60 and 604800),
    period_budget_micros bigint check (period_budget_micros >= 0),
    run_reservation_micros bigint not null default 0 check (run_reservation_micros >= 0),
    members_may_launch boolean not null default false,
    -- Enabling requires every limit, so paid work is never unbounded by omission.
    check (not managed_enabled or (managed_max_concurrent is not null and managed_max_duration_seconds is not null and period_budget_micros is not null))
);
create table if not exists gah_private.admissions (
    workspace_id uuid not null, work_id text not null, payer_id uuid, reserved_micros bigint not null,
    created_at timestamptz not null default now(),
    primary key (workspace_id, work_id),
    foreign key (workspace_id, work_id) references public.work_items on delete cascade
);
alter table gah_private.cloud_limits enable row level security;
alter table gah_private.admissions enable row level security;
revoke all on gah_private.cloud_limits, gah_private.admissions from public, anon, authenticated;
grant all on gah_private.cloud_limits, gah_private.admissions to service_role;
alter table public.work_items add column if not exists max_duration_seconds integer;

-- Month-to-date spend is read from run.usage events, so while managed access is enabled event retention must cover a
-- whole month. Both setters enforce it: enabling here, and shortening retention through the workspaces trigger below.
create or replace function public.configure_cloud_limits(p_workspace_id uuid, p_enabled boolean, p_max_concurrent integer, p_max_duration_seconds integer,
    p_budget_micros bigint, p_run_reservation_micros bigint, p_members_may_launch boolean)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    if coalesce(p_enabled, false) and (select event_retention_days from public.workspaces where id = p_workspace_id) < 31 then
        raise exception 'event_retention_too_short' using errcode = '22023'; end if;
    insert into gah_private.cloud_limits as l (workspace_id, managed_enabled, managed_max_concurrent, managed_max_duration_seconds, period_budget_micros, run_reservation_micros, members_may_launch)
    values (p_workspace_id, coalesce(p_enabled, false), p_max_concurrent, p_max_duration_seconds, p_budget_micros, coalesce(p_run_reservation_micros, 0), coalesce(p_members_may_launch, false))
    on conflict (workspace_id) do update set managed_enabled = excluded.managed_enabled, managed_max_concurrent = excluded.managed_max_concurrent,
        managed_max_duration_seconds = excluded.managed_max_duration_seconds, period_budget_micros = excluded.period_budget_micros,
        run_reservation_micros = excluded.run_reservation_micros, members_may_launch = excluded.members_may_launch;
end;
$$;

create or replace function gah_private.managed_allowance(p_workspace_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare l gah_private.cloud_limits; start_at timestamptz := date_trunc('month', statement_timestamp() at time zone 'UTC') at time zone 'UTC';
    live integer; reported numeric; estimated numeric; held numeric; unknown_runs integer; seconds numeric;
begin
    select * into l from gah_private.cloud_limits where workspace_id = p_workspace_id;
    select count(*) into live from public.work_items w join public.nodes n on n.workspace_id = w.workspace_id and n.node_id = w.node_id and n.worker_kind = 'managed'
        where w.workspace_id = p_workspace_id and (w.state = 'queued' or (w.state = 'running' and w.leased_until > statement_timestamp()));
    select coalesce(sum(r.rep), 0), coalesce(sum(r.est), 0),
        coalesce(sum(case when r.live then greatest(r.reserved_micros - r.rep - r.est, 0) when r.started_at is not null and r.cost_events = 0 then r.reserved_micros else 0 end), 0),
        count(*) filter (where not r.live and r.started_at is not null and r.cost_events = 0),
        coalesce(sum(extract(epoch from coalesce(r.ended_at, statement_timestamp()) - r.started_at)), 0)
    into reported, estimated, held, unknown_runs, seconds from (
        select a.reserved_micros, w.started_at,
            (w.state = 'queued' or (w.state = 'running' and w.leased_until > statement_timestamp())) as live,
            case when w.state in ('queued', 'running') then null else w.updated_at end as ended_at,
            coalesce(sum(gah_private.usage_number(e.payload, 'cost_micros')) filter (where e.payload->'cost_estimated' is distinct from 'true'::jsonb), 0) as rep,
            coalesce(sum(gah_private.usage_number(e.payload, 'cost_micros')) filter (where e.payload->'cost_estimated' = 'true'::jsonb), 0) as est,
            count(e.event_id) as cost_events
        from gah_private.admissions a join public.work_items w on w.workspace_id = a.workspace_id and w.work_id = a.work_id
        left join public.events e on e.workspace_id = a.workspace_id and e.work_id = a.work_id and e.kind = 'run.usage'
            and gah_private.usage_number(e.payload, 'cost_micros') is not null
        where a.workspace_id = p_workspace_id and a.created_at >= start_at
        group by a.workspace_id, a.work_id, a.reserved_micros, w.started_at, w.state, w.leased_until, w.updated_at) r;
    return jsonb_build_object('managed_enabled', coalesce(l.managed_enabled, false), 'period_start', start_at,
        'concurrency', jsonb_build_object('limit', l.managed_max_concurrent, 'in_use', live),
        'max_duration_seconds', l.managed_max_duration_seconds, 'run_reservation_micros', coalesce(l.run_reservation_micros, 0),
        'members_may_launch', coalesce(l.members_may_launch, false),
        'budget', jsonb_build_object('limit_micros', l.period_budget_micros, 'reported_micros', reported, 'estimated_micros', estimated, 'reserved_or_unknown_micros', held,
            'remaining_micros', l.period_budget_micros - reported - estimated - held, 'unknown_cost_runs', unknown_runs),
        'compute_seconds', round(seconds));
end;
$$;
revoke all on function gah_private.managed_allowance(uuid) from public, anon, authenticated;

create or replace function gah_private.metering_retention() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
    if new.event_retention_days < 31 and exists (select 1 from gah_private.cloud_limits where workspace_id = new.id and managed_enabled) then
        raise exception 'event_retention_too_short' using errcode = '22023'; end if;
    return new;
end;
$$;
revoke all on function gah_private.metering_retention() from public, anon, authenticated;
drop trigger if exists metering_retention on public.workspaces;
create trigger metering_retention before update of event_retention_days on public.workspaces for each row execute function gah_private.metering_retention();

create or replace function public.cloud_allowance(p_workspace_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    return gah_private.managed_allowance(p_workspace_id);
end;
$$;

create or replace function gah_private.admit_managed_work() returns trigger
language plpgsql security definer set search_path = '' as $$
declare l gah_private.cloud_limits; a jsonb;
begin
    if not exists (select 1 from public.nodes where workspace_id = new.workspace_id and node_id = new.node_id and worker_kind = 'managed') then return null; end if;
    select * into l from gah_private.cloud_limits where workspace_id = new.workspace_id for update;
    if not found or not l.managed_enabled then raise exception 'managed_not_enabled' using errcode = '42501'; end if;
    if not (l.members_may_launch or gah_private.workspace_owner(new.workspace_id)) then raise exception 'launch_not_permitted' using errcode = '42501'; end if;
    a := gah_private.managed_allowance(new.workspace_id);
    -- This run is already inserted and live, so it is counted in use and its reservation is held.
    if (a->'concurrency'->>'in_use')::integer > l.managed_max_concurrent then raise exception 'concurrency_limit' using errcode = '53400', detail = a::text; end if;
    if (a->'budget'->>'remaining_micros')::numeric - l.run_reservation_micros < 0 then raise exception 'budget_exhausted' using errcode = '53400', detail = a::text; end if;
    insert into gah_private.admissions (workspace_id, work_id, payer_id, reserved_micros) values (new.workspace_id, new.work_id, new.requester_id, l.run_reservation_micros);
    update public.work_items set max_duration_seconds = l.managed_max_duration_seconds where workspace_id = new.workspace_id and work_id = new.work_id;
    return null;
end;
$$;
revoke all on function gah_private.admit_managed_work() from public, anon, authenticated;
drop trigger if exists admit_managed_work on public.work_items;
create trigger admit_managed_work after insert on public.work_items for each row execute function gah_private.admit_managed_work();

-- An owner records a measured cost for a run that never reported one, such as a crashed worker.
create or replace function public.reconcile_cloud_run_cost(p_workspace_id uuid, p_work_id text, p_cost_micros bigint)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.workspace_owner(p_workspace_id) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    if p_cost_micros is null or p_cost_micros < 0 or not exists (select 1 from gah_private.admissions where workspace_id = p_workspace_id and work_id = p_work_id) then
        raise exception 'invalid_reconciliation' using errcode = '22023'; end if;
    insert into public.events (workspace_id, event_id, work_id, kind, actor_id, payload)
    values (p_workspace_id, gen_random_uuid()::text, p_work_id, 'run.usage', auth.uid(), jsonb_build_object('cost_micros', p_cost_micros, 'reason', 'owner reconciliation'));
end;
$$;

-- The run ends at its admitted duration limit, at its next renewal or ownership check.
create or replace function public.renew_cloud_work(p_device_token text, p_work_id text, p_claim_id uuid, p_start boolean default false)
returns boolean language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; over boolean;
begin
    select * into w from public.work_items where workspace_id = d.workspace_id and work_id = p_work_id
        and node_id = d.node_id and device_id = d.id and claim_id = p_claim_id and state = 'running'
        and leased_until > statement_timestamp() for update;
    if not found then return false; end if;
    over := w.started_at is not null and w.max_duration_seconds is not null and w.started_at + make_interval(secs => w.max_duration_seconds) <= statement_timestamp();
    if over or w.stop_requested_at is not null or not gah_private.job_allowed(w) then
        update public.work_items set state = 'cancelled', leased_until = null, updated_at = statement_timestamp()
        where workspace_id = w.workspace_id and work_id = w.work_id;
        perform gah_private.audit(w, case when over then 'cloud.duration_limit' when w.stop_requested_at is not null then 'cloud.handoff_stopped' else 'cloud.access_revoked' end);
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

create or replace function public.check_cloud_ownership(p_device_token text, p_work_id text, p_claim_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; why text;
begin
    select * into w from public.work_items where workspace_id = d.workspace_id and work_id = p_work_id and node_id = d.node_id
        and device_id = d.id and claim_id = p_claim_id;
    why := case when not found or w.state <> 'running' then 'not_running'
        when w.leased_until <= statement_timestamp() then 'lease_expired'
        when w.stop_requested_at is not null then 'stop_requested'
        when w.started_at is not null and w.max_duration_seconds is not null and w.started_at + make_interval(secs => w.max_duration_seconds) <= statement_timestamp() then 'duration_limit'
        when not gah_private.job_allowed(w) then 'access_revoked'
        when w.task_id is not null and w.task_generation is distinct from (select generation from public.tasks where workspace_id = w.workspace_id and task_id = w.task_id) then 'superseded'
    end;
    return jsonb_build_object('owned', why is null, 'reason', why, 'generation', w.task_generation, 'lease_expires_at', w.leased_until);
end;
$$;

-- Placement (ORG-12) excludes a managed worker that admission would refuse, with the admission reason.
create or replace function gah_private.placement(p_workspace_id uuid, p_profile text, p_mode text, p_target integer, p_memory_mb integer,
    p_location text, p_inference_node_id text, p_inference_model text)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare n public.nodes; reasons text[]; running integer; repo uuid; rows jsonb := '[]'; busy_task boolean := false; me uuid := auth.uid(); l gah_private.cloud_limits; allowance jsonb;
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
        -- A managed worker is eligible only when admission would accept the run (ORG-14), so placement never picks a paid
        -- worker that will refuse while an existing worker could take the work.
        if n.worker_kind = 'managed' then
            select * into l from gah_private.cloud_limits where workspace_id = n.workspace_id;
            if not found or not l.managed_enabled then reasons := array_append(reasons, 'managed_not_enabled'::text);
            else
                if not (l.members_may_launch or gah_private.workspace_owner(n.workspace_id)) then reasons := array_append(reasons, 'launch_not_permitted'::text); end if;
                allowance := coalesce(allowance, gah_private.managed_allowance(n.workspace_id));
                if (allowance->'concurrency'->>'in_use')::integer >= l.managed_max_concurrent then reasons := array_append(reasons, 'concurrency_limit'::text); end if;
                if (allowance->'budget'->>'remaining_micros')::numeric - l.run_reservation_micros < 0 then reasons := array_append(reasons, 'budget_exhausted'::text); end if;
            end if;
        end if;
        rows := rows || jsonb_build_object('node_id', n.node_id, 'worker_kind', n.worker_kind, 'eligible', cardinality(reasons) = 0, 'reasons', to_jsonb(reasons),
            'running', running, 'max_concurrent', n.max_concurrent, 'queued_reason', case when cardinality(reasons) = 0 and running >= n.max_concurrent then 'at_capacity' end);
    end loop;
    return jsonb_build_object('candidates', rows, 'chosen', (select c->>'node_id' from jsonb_array_elements(rows) c where (c->>'eligible')::boolean
        order by (c->>'running')::numeric / greatest((c->>'max_concurrent')::numeric, 1), c->>'node_id' limit 1));
end;
$$;

-- Erasure (ORG-10) also removes the departed member as the payer of their admissions.
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
    update public.tasks set handoff_kind = null, handoff_to = null, handoff_expires_at = null
        where workspace_id = p_workspace_id and handoff_to = p_user_id;
    return jsonb_build_object('events', e, 'audit', a, 'runs', r);
end;
$$;

-- The audit log records limit changes with the other administrative settings.
create or replace function gah_private.audit_change() returns trigger
language plpgsql security definer set search_path = '' as $$
declare r jsonb := to_jsonb(coalesce(new, old)); picked jsonb;
begin
    select coalesce(jsonb_object_agg(case when k = 'id' then 'device_id' else k end, v), '{}') into picked from jsonb_each(r) e(k, v)
        where k = any (array['user_id', 'role', 'node_id', 'resource', 'permission', 'sharing_scope', 'inference_scope', 'repo_id',
            'coordinated', 'visibility', 'event_retention_days', 'audit_retention_days', 'revoked_at', 'managed_enabled', 'managed_max_concurrent',
            'managed_max_duration_seconds', 'period_budget_micros', 'run_reservation_micros', 'members_may_launch'])
        or (k = 'id' and tg_table_name = 'devices');
    insert into gah_private.audit_log (workspace_id, actor_id, action, target)
    values (coalesce(r->>'workspace_id', r->>'id')::uuid, auth.uid(), tg_table_name || '.' || lower(tg_op), picked);
    return null;
end;
$$;
revoke all on function gah_private.audit_change() from public, anon, authenticated;
drop trigger if exists audit_cloud_limits on gah_private.cloud_limits;
create trigger audit_cloud_limits after insert or update or delete on gah_private.cloud_limits for each row execute function gah_private.audit_change();

revoke all on function public.configure_cloud_limits(uuid, boolean, integer, integer, bigint, bigint, boolean), public.cloud_allowance(uuid),
    public.reconcile_cloud_run_cost(uuid, text, bigint) from public, anon;
grant execute on function public.configure_cloud_limits(uuid, boolean, integer, integer, bigint, bigint, boolean), public.cloud_allowance(uuid),
    public.reconcile_cloud_run_cost(uuid, text, bigint) to authenticated;

commit;
