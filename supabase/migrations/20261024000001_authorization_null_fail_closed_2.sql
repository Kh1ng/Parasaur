-- NULL requester and lease values must never grant worker or run authority.
-- Preserve applied migrations; replace only the denial predicates.
begin;

create or replace function public.cancel_managed_worker(p_workspace_id uuid, p_worker_id uuid)
returns text language plpgsql security definer set search_path = '' as $$
declare w gah_private.managed_workers;
begin
    select * into w from gah_private.managed_workers where workspace_id = p_workspace_id and worker_id = p_worker_id for update;
    if not found or not gah_private.can_access(p_workspace_id) or not (w.requested_by = auth.uid() or gah_private.workspace_owner(p_workspace_id)) is true then
        raise exception 'worker_write_denied' using errcode = '42501'; end if;
    if w.state in ('stopping', 'retired') then return w.state; end if;
    update gah_private.managed_workers set state = case when state = 'requested' and provider_ref is null and device_id is null then 'retired' else 'stopping' end,
        cleanup_confirmed = state = 'requested' and provider_ref is null and device_id is null, updated_at = statement_timestamp()
    where workspace_id = p_workspace_id and worker_id = p_worker_id returning state into w.state;
    if w.device_id is not null then update gah_private.devices set revoked_at = coalesce(revoked_at, statement_timestamp()) where id = w.device_id; end if;
    return w.state;
end;
$$;

create or replace function public.check_cloud_ownership(p_device_token text, p_work_id text, p_claim_id uuid)
returns jsonb language plpgsql stable security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); w public.work_items; why text;
begin
    select * into w from public.work_items where workspace_id = d.workspace_id and work_id = p_work_id and node_id = d.node_id
        and device_id = d.id and claim_id = p_claim_id;
    why := case when not found or w.state <> 'running' then 'not_running'
        when not (w.leased_until > statement_timestamp()) is true then 'lease_expired'
        when w.stop_requested_at is not null then 'stop_requested'
        when w.started_at is not null and w.max_duration_seconds is not null and w.started_at + make_interval(secs => w.max_duration_seconds) <= statement_timestamp() then 'duration_limit'
        when not gah_private.job_allowed(w) then 'access_revoked'
        when w.task_id is not null and w.task_generation is distinct from (select generation from public.tasks where workspace_id = w.workspace_id and task_id = w.task_id) then 'superseded'
    end;
    return jsonb_build_object('owned', why is null, 'reason', why, 'generation', w.task_generation, 'lease_expires_at', w.leased_until);
end;
$$;

create or replace function public.poll_cloud_inference(p_device_token text, p_request_id uuid)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); r gah_private.inference_requests; w public.work_items;
begin
    select * into r from gah_private.inference_requests where id = p_request_id and compute_device_id = d.id and expires_at > statement_timestamp();
    if not found then raise exception 'inference_access_denied' using errcode = '42501'; end if;
    select * into w from public.work_items where workspace_id = r.workspace_id and work_id = r.work_id;
    if not (w.state = 'running' and w.leased_until > statement_timestamp() and gah_private.job_allowed(w)) is true then
        raise exception 'inference_access_denied' using errcode = '42501'; end if;
    if r.state in ('completed', 'failed') then
        delete from gah_private.inference_requests where id = r.id;
        return jsonb_build_object('status', r.status, 'response', r.response);
    end if;
    return null;
end;
$$;

commit;
