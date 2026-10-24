-- ORG-08: organization metrics computed from run records and ORG-03 events, one interpretation of each outcome.
-- Window: a half-open [from, to) interval on the time a run reached a terminal state (work_items.updated_at, which no
--   function changes after a run is terminal). Runs still queued or running are counted as a snapshot, never in totals.
-- Outcome: work_items.state. completed means the agent run exited 0. It does not mean merged or accepted work, which GAH
--   does not record, so no such measure exists here.
-- completion_rate = completed / (completed + failed); cancelled runs are excluded from the denominator.
-- Times: wait is queued_at to started_at, execution is started_at to terminal. Runs that never started have neither.
-- repeat_attempts: runs on a task that already had an earlier run (retries, handoffs, takeovers all count).
-- Usage: each run.usage event is additive for its run. Late events change the totals of the window their run ended in.
--   Reported cost (cost_estimated false or absent) and estimated cost are separate sums. A started run with no cost event
--   is counted in unknown_cost_runs; unknown cost is never zero. Coverage is limited by event retention (ORG-10).
-- Permissions: every figure covers only runs whose repository the caller may see (ORG-10). Runs with no repository are
--   visible to owners, to their requester, and to everyone in a workspace-visibility workspace. by_member lists every
--   member for owners and only the caller for others; a null requester is a former member (erased under ORG-10).
-- ownership_conflicts counts only events of runs the caller may see (and, with a repository filter, of that repository).
-- Not measured: denied claim and lock attempts raise errors or return results without an event, so ownership_conflicts
--   counts only handoff acceptances, takeovers, and lock resolutions.
begin;

-- Usage figures are whole non-negative numbers and cost_estimated is a boolean. report_cloud_event enforces this at
-- ingestion; readers still read through gah_private.usage_number so a malformed stored row counts as absent, not as an error.
create or replace function public.report_cloud_event(p_device_token text, p_work_id text, p_claim_id uuid, p_run_seq integer, p_kind text, p_payload jsonb)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; prior public.events; e public.events; k text; v jsonb; bad boolean;
begin
    if p_kind is null or p_kind not in ('run.state', 'run.handoff', 'run.outcome', 'run.usage')
        or p_run_seq is null or p_run_seq not between 1 and 10000 or jsonb_typeof(p_payload) is distinct from 'object' then
        raise exception 'invalid_event' using errcode = '22023'; end if;
    for k, v in select * from jsonb_each(p_payload) loop
        if k <> all (array['state', 'reason', 'exit_code', 'model', 'provider', 'target', 'input_tokens', 'output_tokens', 'cost_micros', 'cost_estimated'])
            or jsonb_typeof(v) not in ('string', 'number', 'boolean', 'null') or length(v::text) > 202 then
            raise exception 'invalid_event_payload' using errcode = '22023'; end if;
        -- Usage figures are summed by metrics, metering and reports, so they must be whole non-negative numbers.
        bad := case when k in ('input_tokens', 'output_tokens', 'cost_micros') then
                case when jsonb_typeof(v) = 'number' then v::text::numeric < 0 or v::text::numeric <> trunc(v::text::numeric) else true end
            when k = 'cost_estimated' then jsonb_typeof(v) <> 'boolean' else false end;
        if bad then
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

create or replace function gah_private.usage_number(p jsonb, k text) returns numeric
language sql immutable set search_path = '' as $$
    select case when jsonb_typeof(p->k) = 'number' then case when (p->>k)::numeric >= 0 then (p->>k)::numeric end end;
$$;

-- Existing rows keep a null queued_at, so they have no wait time rather than a wrong one.
alter table public.work_items add column if not exists queued_at timestamptz;
alter table public.work_items alter column queued_at set default now();

create or replace function public.cloud_org_metrics(p_workspace_id uuid, p_from timestamptz, p_to timestamptz, p_repo_id uuid default null)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare me uuid := auth.uid(); is_owner boolean; vis text; result jsonb;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    if p_from is null or p_to is null or p_from >= p_to or p_to - p_from > interval '366 days' then raise exception 'invalid_window' using errcode = '22023'; end if;
    is_owner := gah_private.workspace_owner(p_workspace_id);
    select visibility into vis from public.workspaces where id = p_workspace_id;
    with runs as (
        select w.work_id, w.requester_id, t.repo_id, w.state, w.queued_at, w.started_at, w.updated_at as finished_at, coalesce(w.task_generation, 1) > 1 as repeat_attempt,
            (w.state in ('queued', 'running')) as live
        from public.work_items w left join public.tasks t on t.workspace_id = w.workspace_id and t.task_id = w.task_id
        where w.workspace_id = p_workspace_id and (p_repo_id is null or t.repo_id = p_repo_id)
            and case when t.repo_id is not null then gah_private.repo_visible(w.workspace_id, t.repo_id, me)
                else is_owner or w.requester_id = me or vis = 'workspace' end
    ), done as (
        select * from runs where not live and finished_at >= p_from and finished_at < p_to
    ), usage as (
        select d.work_id, d.requester_id, d.repo_id,
            sum(gah_private.usage_number(e.payload, 'input_tokens')) as input_tokens, sum(gah_private.usage_number(e.payload, 'output_tokens')) as output_tokens,
            sum(gah_private.usage_number(e.payload, 'cost_micros')) filter (where e.payload->'cost_estimated' is distinct from 'true'::jsonb) as reported_cost,
            sum(gah_private.usage_number(e.payload, 'cost_micros')) filter (where e.payload->'cost_estimated' = 'true'::jsonb) as estimated_cost,
            count(gah_private.usage_number(e.payload, 'cost_micros')) as cost_events, count(*) as usage_events
        from done d join public.events e on e.workspace_id = p_workspace_id and e.work_id = d.work_id and e.kind = 'run.usage' group by 1, 2, 3
    ), per_run as (
        select d.*, u.input_tokens, u.output_tokens, u.reported_cost, u.estimated_cost, coalesce(u.cost_events, 0) as cost_events, coalesce(u.usage_events, 0) as usage_events
        from done d left join usage u using (work_id)
    ), summarize as (
        select jsonb_build_object(
            'runs', count(*), 'completed', count(*) filter (where state = 'completed'), 'failed', count(*) filter (where state = 'failed'),
            'cancelled', count(*) filter (where state = 'cancelled'),
            'completion_rate', round((count(*) filter (where state = 'completed'))::numeric / nullif(count(*) filter (where state in ('completed', 'failed')), 0), 4),
            'repeat_attempts', count(*) filter (where repeat_attempt),
            'avg_wait_seconds', round(avg(extract(epoch from started_at - queued_at))::numeric, 1),
            'avg_execution_seconds', round(avg(extract(epoch from finished_at - started_at))::numeric, 1),
            'input_tokens', coalesce(sum(input_tokens), 0), 'output_tokens', coalesce(sum(output_tokens), 0),
            'runs_with_usage', count(*) filter (where usage_events > 0),
            'reported_cost_micros', sum(reported_cost), 'estimated_cost_micros', sum(estimated_cost),
            'unknown_cost_runs', count(*) filter (where started_at is not null and cost_events = 0)) as j
        from per_run
    )
    select jsonb_build_object(
        'window', jsonb_build_object('from', p_from, 'to', p_to, 'basis', 'terminal time, half-open'),
        'generated_at', statement_timestamp(),
        'coverage', jsonb_build_object('oldest_event_at', (select min(occurred_at) from public.events where workspace_id = p_workspace_id),
            'event_retention_days', (select event_retention_days from public.workspaces where id = p_workspace_id)),
        'snapshot', jsonb_build_object('queued', (select count(*) from runs where state = 'queued'), 'running', (select count(*) from runs where state = 'running')),
        'totals', (select j from summarize),
        'ownership_conflicts', (select count(*) from public.events ev where ev.workspace_id = p_workspace_id and ev.kind in ('task.handoff_accepted', 'task.takeover', 'file.resolved')
            and ev.occurred_at >= p_from and ev.occurred_at < p_to and ev.work_id in (select work_id from runs)),
        'by_repository', coalesce((select jsonb_agg(jsonb_build_object('repo_id', repo_id, 'runs', n, 'completed', c, 'failed', f) order by repo_id) from (
            select repo_id, count(*) n, count(*) filter (where state = 'completed') c, count(*) filter (where state = 'failed') f from done group by repo_id) r), '[]'),
        'by_model', coalesce((select jsonb_agg(jsonb_build_object('provider', provider, 'model', model, 'input_tokens', i, 'output_tokens', o) order by provider, model) from (
            select e.payload->>'provider' provider, e.payload->>'model' model, sum(gah_private.usage_number(e.payload, 'input_tokens')) i, sum(gah_private.usage_number(e.payload, 'output_tokens')) o
            from done d join public.events e on e.workspace_id = p_workspace_id and e.work_id = d.work_id and e.kind = 'run.usage' group by 1, 2) m), '[]'),
        'by_member', coalesce((select jsonb_agg(jsonb_build_object('user_id', requester_id, 'runs', n, 'completed', c, 'failed', f, 'input_tokens', i, 'output_tokens', o) order by requester_id) from (
            select d.requester_id, count(*) n, count(*) filter (where d.state = 'completed') c, count(*) filter (where d.state = 'failed') f,
                coalesce(sum(p.input_tokens), 0) i, coalesce(sum(p.output_tokens), 0) o
            from done d join per_run p using (work_id) where is_owner or d.requester_id = me group by d.requester_id) mm), '[]'))
    into result;
    return result;
end;
$$;
revoke all on function public.cloud_org_metrics(uuid, timestamptz, timestamptz, uuid) from public, anon;
grant execute on function public.cloud_org_metrics(uuid, timestamptz, timestamptz, uuid) to authenticated;

commit;
