-- ORG-05 follow-up: an accepted handoff or takeover must stop the prior run even when the run is requeued at the same
-- moment. The handoff now locks the live run it read, so a concurrent release or lease reclaim either finishes first and
-- is seen, or waits until the stop has landed. A stopped run that would return to the queue is cancelled instead, so a
-- stop that lands first is never undone by a later requeue.
begin;

-- Lock order is task, then run, as in enqueue_cloud_work.
create or replace function gah_private.handoff_context(p_workspace_id uuid, p_task_id uuid, out t public.tasks, out w public.work_items)
language plpgsql security definer set search_path = '' as $$
begin
    if not gah_private.can_access(p_workspace_id) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
    select * into t from public.tasks where workspace_id = p_workspace_id and task_id = p_task_id for update;
    if not found then raise exception 'task_not_found' using errcode = '22023'; end if;
    select * into w from public.work_items where workspace_id = p_workspace_id and task_id = p_task_id and state in ('queued', 'running') for update;
    if t.handoff_kind is not null and (t.handoff_expires_at <= statement_timestamp() or (t.handoff_kind <> 'approved' and w.work_id is null)) then
        update public.tasks set handoff_kind = null, handoff_to = null, handoff_expires_at = null where workspace_id = t.workspace_id and task_id = t.task_id
            returning * into t;
    end if;
end;
$$;
revoke all on function gah_private.handoff_context(uuid, uuid) from public, anon, authenticated;

-- Every requeue path (capacity release, lease reclaim at claim or enqueue) goes through this.
create or replace function gah_private.stopped_never_requeued() returns trigger
language plpgsql security definer set search_path = '' as $$
declare w public.work_items := new;
begin
    new.state := 'cancelled';
    -- Attribute the stop to the session that held the run, which the requeue has just cleared.
    w.state := 'cancelled'; w.device_id := old.device_id; w.agent_session_id := old.agent_session_id;
    perform gah_private.audit(w, 'cloud.handoff_stopped');
    return new;
end;
$$;
revoke all on function gah_private.stopped_never_requeued() from public, anon, authenticated;
drop trigger if exists stopped_never_requeued on public.work_items;
create trigger stopped_never_requeued before update of state on public.work_items for each row
    when (new.state = 'queued' and old.state <> 'queued' and new.stop_requested_at is not null) execute function gah_private.stopped_never_requeued();

-- The release event says what happened to the run: deferred, or stopped by a handoff.
create or replace function public.release_cloud_work(p_device_token text, p_work_id text, p_claim_id uuid)
returns boolean language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; held public.work_items;
begin
    select * into held from public.work_items where workspace_id = d.workspace_id and node_id = d.node_id and work_id = p_work_id
        and device_id = d.id and claim_id = p_claim_id and state = 'running' and started_at is null for update;
    if not found then return false; end if;
    update public.work_items set state = 'queued', device_id = null, claim_id = null, agent_session_id = null,
        leased_until = null, updated_at = statement_timestamp()
    where workspace_id = held.workspace_id and work_id = held.work_id returning * into w;
    if w.state = 'queued' then
        w.device_id := held.device_id; w.agent_session_id := held.agent_session_id;
        perform gah_private.audit(w, 'cloud.capacity_deferred');
    end if;
    return true;
end;
$$;

commit;
