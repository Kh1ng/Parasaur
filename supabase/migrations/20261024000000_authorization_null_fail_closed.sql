-- NULL node permissions must deny takeover and orphan-lock resolution.
-- Preserve applied migrations and replace only their authorization predicates.
begin;

create or replace function public.force_cloud_takeover(p_workspace_id uuid, p_task_id uuid, p_to uuid default null)
returns void language plpgsql security definer set search_path = '' as $$
declare h record; t public.tasks; w public.work_items; target uuid := coalesce(p_to, auth.uid());
begin
    select * into h from gah_private.handoff_context(p_workspace_id, p_task_id); t := h.t; w := h.w;
    if w.work_id is null then raise exception 'task_not_claimed' using errcode = '22023'; end if;
    if not (gah_private.workspace_owner(p_workspace_id) or gah_private.node_permission(p_workspace_id, w.node_id) in ('owner', 'manage')) is true then
        raise exception 'takeover_denied' using errcode = '42501'; end if;
    if not gah_private.member_active(p_workspace_id, target) then raise exception 'invalid_handoff_target' using errcode = '22023'; end if;
    perform gah_private.stop_run(w);
    update public.tasks set handoff_kind = 'approved', handoff_to = target, handoff_expires_at = statement_timestamp() + interval '10 minutes'
        where workspace_id = p_workspace_id and task_id = p_task_id;
    perform gah_private.task_event(t, w.work_id, 'task.takeover', jsonb_build_object('to', target, 'from', w.requester_id));
end;
$$;

create or replace function public.resolve_cloud_file_locks(p_workspace_id uuid, p_work_id text, p_disposition text)
returns integer language plpgsql security definer set search_path = '' as $$
declare w public.work_items; n integer;
begin
    if p_disposition is null or p_disposition not in ('preserved', 'discarded', 'handed_off') then raise exception 'invalid_disposition' using errcode = '22023'; end if;
    select * into w from public.work_items where workspace_id = p_workspace_id and work_id = p_work_id for update;
    if not found or not gah_private.can_access(p_workspace_id) or not (w.requester_id = auth.uid() or gah_private.workspace_owner(p_workspace_id)
        or gah_private.node_permission(p_workspace_id, w.node_id) in ('owner', 'manage')) is true then raise exception 'resolve_denied' using errcode = '42501'; end if;
    if w.state = 'queued' or (w.state = 'running' and w.leased_until > statement_timestamp()) then raise exception 'run_still_active' using errcode = '55006'; end if;
    update public.file_locks set resolved_at = statement_timestamp(), resolution = p_disposition
        where workspace_id = p_workspace_id and work_id = p_work_id and resolved_at is null;
    get diagnostics n = row_count;
    insert into public.events (workspace_id, event_id, work_id, kind, actor_id, payload)
    values (p_workspace_id, gen_random_uuid()::text, p_work_id, 'file.resolved', auth.uid(), jsonb_build_object('disposition', p_disposition, 'count', n));
    return n;
end;
$$;

commit;
