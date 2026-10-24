-- ORG-09 (server side): report drill-down, filters, and export that share one run set with the ORG-08 totals.
-- gah_private.metric_runs is the only place that decides which runs a caller may count and what each run's outcome,
-- times, and usage are. cloud_org_metrics, the drill-down, and the export all read it, so displayed and exported totals
-- cannot disagree. A null window returns live (queued or running) runs for the snapshot.
-- Hidden repositories never appear: not in totals, rows, filter choices, or exports. A member sees names only on their
-- own runs; owners see everyone's. Agent activity is not a measure of anyone's productivity, and nothing here ranks people.
begin;

create or replace function gah_private.metric_runs(p_workspace_id uuid, p_from timestamptz, p_to timestamptz, p_repo_id uuid)
returns table (work_id text, requester_id uuid, repo_id uuid, state text, queued_at timestamptz, started_at timestamptz, finished_at timestamptz, repeat_attempt boolean,
    input_tokens numeric, output_tokens numeric, reported_cost numeric, estimated_cost numeric, cost_events bigint, usage_events bigint)
language plpgsql stable security definer set search_path = '' as $$
declare me uuid := auth.uid(); is_owner boolean := gah_private.workspace_owner(p_workspace_id); vis text;
begin
    select visibility into vis from public.workspaces where id = p_workspace_id;
    return query
    with runs as (
        select w.work_id, w.requester_id, t.repo_id, w.state, w.queued_at, w.started_at, w.updated_at as finished_at, coalesce(w.task_generation, 1) > 1 as repeat_attempt
        from public.work_items w left join public.tasks t on t.workspace_id = w.workspace_id and t.task_id = w.task_id
        where w.workspace_id = p_workspace_id and (p_repo_id is null or t.repo_id = p_repo_id)
            and case when t.repo_id is not null then gah_private.repo_visible(w.workspace_id, t.repo_id, me)
                else is_owner or w.requester_id = me or vis = 'workspace' end
            and case when p_from is null then w.state in ('queued', 'running')
                else w.state not in ('queued', 'running') and w.updated_at >= p_from and w.updated_at < p_to end
    )
    select r.work_id, r.requester_id, r.repo_id, r.state, r.queued_at, r.started_at, r.finished_at, r.repeat_attempt,
        sum(gah_private.usage_number(e.payload, 'input_tokens')), sum(gah_private.usage_number(e.payload, 'output_tokens')),
        sum(gah_private.usage_number(e.payload, 'cost_micros')) filter (where e.payload->'cost_estimated' is distinct from 'true'::jsonb),
        sum(gah_private.usage_number(e.payload, 'cost_micros')) filter (where e.payload->'cost_estimated' = 'true'::jsonb),
        count(gah_private.usage_number(e.payload, 'cost_micros')), count(e.event_id)
    from runs r left join public.events e on e.workspace_id = p_workspace_id and e.work_id = r.work_id and e.kind = 'run.usage'
    group by r.work_id, r.requester_id, r.repo_id, r.state, r.queued_at, r.started_at, r.finished_at, r.repeat_attempt;
end;
$$;
revoke all on function gah_private.metric_runs(uuid, timestamptz, timestamptz, uuid) from public, anon, authenticated;

-- ORG-08 totals, now read from the shared run set. Output is unchanged.
create or replace function public.cloud_org_metrics(p_workspace_id uuid, p_from timestamptz, p_to timestamptz, p_repo_id uuid default null)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare me uuid := auth.uid(); is_owner boolean; vis text; result jsonb;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    if p_from is null or p_to is null or p_from >= p_to or p_to - p_from > interval '366 days' then raise exception 'invalid_window' using errcode = '22023'; end if;
    is_owner := gah_private.workspace_owner(p_workspace_id);
    select visibility into vis from public.workspaces where id = p_workspace_id;
    select jsonb_build_object(
        'window', jsonb_build_object('from', p_from, 'to', p_to, 'basis', 'terminal time, half-open'),
        'generated_at', statement_timestamp(),
        'coverage', jsonb_build_object('oldest_event_at', (select min(occurred_at) from public.events where workspace_id = p_workspace_id),
            'event_retention_days', (select event_retention_days from public.workspaces where id = p_workspace_id)),
        'snapshot', (select jsonb_build_object('queued', count(*) filter (where state = 'queued'), 'running', count(*) filter (where state = 'running')) from gah_private.metric_runs(p_workspace_id, null, null, p_repo_id)),
        'totals', (select jsonb_build_object(
            'runs', count(*), 'completed', count(*) filter (where state = 'completed'), 'failed', count(*) filter (where state = 'failed'),
            'cancelled', count(*) filter (where state = 'cancelled'),
            'completion_rate', round((count(*) filter (where state = 'completed'))::numeric / nullif(count(*) filter (where state in ('completed', 'failed')), 0), 4),
            'repeat_attempts', count(*) filter (where repeat_attempt),
            'avg_wait_seconds', round(avg(extract(epoch from started_at - queued_at))::numeric, 1),
            'avg_execution_seconds', round(avg(extract(epoch from finished_at - started_at))::numeric, 1),
            'input_tokens', coalesce(sum(input_tokens), 0), 'output_tokens', coalesce(sum(output_tokens), 0),
            'runs_with_usage', count(*) filter (where usage_events > 0),
            'reported_cost_micros', sum(reported_cost), 'estimated_cost_micros', sum(estimated_cost),
            'unknown_cost_runs', count(*) filter (where started_at is not null and cost_events = 0)) from gah_private.metric_runs(p_workspace_id, p_from, p_to, p_repo_id)),
        'ownership_conflicts', (select count(*) from public.events ev where ev.workspace_id = p_workspace_id and ev.kind in ('task.handoff_accepted', 'task.takeover', 'file.resolved')
            and ev.occurred_at >= p_from and ev.occurred_at < p_to
            -- Only events of runs the caller may see, by the same rule as metric_runs (ORG-10), and of the filtered repository.
            and exists (select 1 from public.work_items w left join public.tasks t on t.workspace_id = w.workspace_id and t.task_id = w.task_id
                where w.workspace_id = ev.workspace_id and w.work_id = ev.work_id and (p_repo_id is null or t.repo_id = p_repo_id)
                    and case when t.repo_id is not null then gah_private.repo_visible(w.workspace_id, t.repo_id, me)
                        else is_owner or w.requester_id = me or vis = 'workspace' end)),
        'by_repository', coalesce((select jsonb_agg(jsonb_build_object('repo_id', repo_id, 'runs', n, 'completed', c, 'failed', f) order by repo_id) from (
            select repo_id, count(*) n, count(*) filter (where state = 'completed') c, count(*) filter (where state = 'failed') f
            from gah_private.metric_runs(p_workspace_id, p_from, p_to, p_repo_id) group by repo_id) r), '[]'),
        'by_model', coalesce((select jsonb_agg(jsonb_build_object('provider', provider, 'model', model, 'input_tokens', i, 'output_tokens', o) order by provider, model) from (
            select e.payload->>'provider' provider, e.payload->>'model' model, sum(gah_private.usage_number(e.payload, 'input_tokens')) i, sum(gah_private.usage_number(e.payload, 'output_tokens')) o
            from gah_private.metric_runs(p_workspace_id, p_from, p_to, p_repo_id) d join public.events e on e.workspace_id = p_workspace_id and e.work_id = d.work_id and e.kind = 'run.usage' group by 1, 2) m), '[]'),
        'by_member', coalesce((select jsonb_agg(jsonb_build_object('user_id', requester_id, 'runs', n, 'completed', c, 'failed', f, 'input_tokens', i, 'output_tokens', o) order by requester_id) from (
            select d.requester_id, count(*) n, count(*) filter (where d.state = 'completed') c, count(*) filter (where d.state = 'failed') f,
                coalesce(sum(d.input_tokens), 0) i, coalesce(sum(d.output_tokens), 0) o
            from gah_private.metric_runs(p_workspace_id, p_from, p_to, p_repo_id) d where is_owner or d.requester_id = me group by d.requester_id) mm), '[]'))
    into result;
    return result;
end;
$$;

-- Choices for report filters: only repositories and models the caller may see, so a filter never reveals a hidden one.
create or replace function public.cloud_report_filters(p_workspace_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    return jsonb_build_object('repositories', coalesce((select jsonb_agg(jsonb_build_object('repo_id', r.repo_id, 'path', r.path) order by r.path) from public.repositories r
        where r.workspace_id = p_workspace_id and gah_private.repo_visible(r.workspace_id, r.repo_id, auth.uid())), '[]'));
end;
$$;

-- Drill-down: the underlying runs of a window, newest first, paged by (finished_at, work_id). Names appear only on the
-- caller's own runs unless the caller is an owner.
create or replace function public.cloud_report_runs(p_workspace_id uuid, p_from timestamptz, p_to timestamptz, p_repo_id uuid default null,
    p_after_time timestamptz default null, p_after_id text default null, p_limit integer default 50)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare is_owner boolean; me uuid := auth.uid();
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    if p_from is null or p_to is null or p_from >= p_to or p_to - p_from > interval '366 days' then raise exception 'invalid_window' using errcode = '22023'; end if;
    is_owner := gah_private.workspace_owner(p_workspace_id);
    return coalesce((select jsonb_agg(to_jsonb(x) order by x.finished_at desc, x.work_id desc) from (
        select d.work_id, d.state, d.finished_at, d.started_at, d.queued_at, r.path as repository, t.kind, t.ref,
            case when is_owner or d.requester_id = me then coalesce(m.display_name, 'former member') end as initiated_by,
            round(extract(epoch from d.started_at - d.queued_at)::numeric, 1) as wait_seconds, round(extract(epoch from d.finished_at - d.started_at)::numeric, 1) as execution_seconds,
            d.input_tokens, d.output_tokens, d.reported_cost as reported_cost_micros, d.estimated_cost as estimated_cost_micros,
            d.started_at is not null and d.cost_events = 0 as cost_unknown, d.repeat_attempt
        from gah_private.metric_runs(p_workspace_id, p_from, p_to, p_repo_id) d
        join public.work_items w on w.workspace_id = p_workspace_id and w.work_id = d.work_id
        left join public.tasks t on t.workspace_id = w.workspace_id and t.task_id = w.task_id
        left join public.repositories r on r.workspace_id = t.workspace_id and r.repo_id = t.repo_id
        left join gah_private.memberships m on m.workspace_id = p_workspace_id and m.user_id = d.requester_id
        where p_after_time is null or (d.finished_at, d.work_id) < (p_after_time, p_after_id)
        order by d.finished_at desc, d.work_id desc limit least(greatest(p_limit, 1), 200)) x), '[]');
end;
$$;

-- Export: the same rows as CSV, up to 10000, with the window and the row count stated on the first line; a larger
-- window says it was truncated, so an export never silently disagrees with the totals. Text that starts like a
-- spreadsheet formula (=, +, -, @, tab or carriage return) gets a leading quote so it is not run.
create or replace function public.cloud_report_export(p_workspace_id uuid, p_from timestamptz, p_to timestamptz, p_repo_id uuid default null)
returns text language plpgsql stable security definer set search_path = '' as $$
declare is_owner boolean; me uuid := auth.uid(); body text; total bigint;
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    if p_from is null or p_to is null or p_from >= p_to or p_to - p_from > interval '366 days' then raise exception 'invalid_window' using errcode = '22023'; end if;
    is_owner := gah_private.workspace_owner(p_workspace_id);
    select coalesce(string_agg(line, E'\n' order by finished_at desc, work_id desc), '') into body from (
        select d.finished_at, d.work_id, concat_ws(',', d.work_id, d.state, to_char(d.finished_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
            '"' || replace(coalesce(case when left(r.path, 1) in ('=', '+', '-', '@', E'\t', E'\r') then '''' || r.path else r.path end, ''), '"', '""') || '"',
            '"' || replace(case when left(n.nm, 1) in ('=', '+', '-', '@', E'\t', E'\r') then '''' || n.nm else n.nm end, '"', '""') || '"',
            coalesce(round(extract(epoch from d.started_at - d.queued_at)::numeric, 1)::text, ''), coalesce(round(extract(epoch from d.finished_at - d.started_at)::numeric, 1)::text, ''),
            coalesce(d.input_tokens::text, ''), coalesce(d.output_tokens::text, ''), coalesce(d.reported_cost::text, ''), coalesce(d.estimated_cost::text, ''),
            case when d.started_at is not null and d.cost_events = 0 then 'true' else 'false' end) as line
        from gah_private.metric_runs(p_workspace_id, p_from, p_to, p_repo_id) d
        join public.work_items w on w.workspace_id = p_workspace_id and w.work_id = d.work_id
        left join public.tasks t on t.workspace_id = w.workspace_id and t.task_id = w.task_id
        left join public.repositories r on r.workspace_id = t.workspace_id and r.repo_id = t.repo_id
        left join gah_private.memberships m on m.workspace_id = p_workspace_id and m.user_id = d.requester_id
        cross join lateral (select coalesce(case when is_owner or d.requester_id = me then coalesce(m.display_name, 'former member') end, '') as nm) n
        order by d.finished_at desc, d.work_id desc limit 10000) x;
    select count(*) into total from gah_private.metric_runs(p_workspace_id, p_from, p_to, p_repo_id);
    return '# window ' || to_char(p_from at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') || ' to ' || to_char(p_to at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"')
        || ' (terminal time, half-open); generated ' || to_char(statement_timestamp() at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"')
        || '; ' || total || ' runs' || case when total > 10000 then '; truncated to the newest 10000, narrow the window for the rest' else '' end || E'\n'
        || 'work_id,state,finished_at,repository,initiated_by,wait_seconds,execution_seconds,input_tokens,output_tokens,reported_cost_micros,estimated_cost_micros,cost_unknown'
        || case when body = '' then '' else E'\n' || body end;
end;
$$;

revoke all on function public.cloud_report_filters(uuid), public.cloud_report_runs(uuid, timestamptz, timestamptz, uuid, timestamptz, text, integer),
    public.cloud_report_export(uuid, timestamptz, timestamptz, uuid) from public, anon;
grant execute on function public.cloud_report_filters(uuid), public.cloud_report_runs(uuid, timestamptz, timestamptz, uuid, timestamptz, text, integer),
    public.cloud_report_export(uuid, timestamptz, timestamptz, uuid) to authenticated;

commit;
