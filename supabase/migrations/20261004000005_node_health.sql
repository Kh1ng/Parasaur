-- TICKET-251: one device-bound, rate-bounded liveness path. Identity comes only from the
-- device credential. Registration never marks a node online; only this function does.
begin;

-- The 90 second lease must match the 'online' window in cloud_node_health (migration 003).
create or replace function public.cloud_node_heartbeat(p_device_token text)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token); n public.nodes; accepted boolean;
begin
    -- Server time only. A beat within 10 seconds of the last one keeps the existing lease.
    update public.nodes set last_seen_at = statement_timestamp(), worker_heartbeat_at = statement_timestamp()
    where workspace_id = d.workspace_id and node_id = d.node_id
        and (worker_heartbeat_at is null or worker_heartbeat_at <= statement_timestamp() - interval '10 seconds')
    returning * into n;
    accepted := found;
    if not accepted then select * into n from public.nodes where workspace_id = d.workspace_id and node_id = d.node_id; end if;
    -- advertised_url is diagnostic only; routing uses the stable workspace and node identity.
    return jsonb_build_object('workspace_id', n.workspace_id, 'node_id', n.node_id, 'accepted', accepted,
        'lease_expires_at', n.worker_heartbeat_at + interval '90 seconds', 'routing_mode', n.transport_mode,
        'min_interval_seconds', 10);
end;
$$;
revoke all on function public.cloud_node_heartbeat(text) from public, anon, authenticated, service_role;
grant execute on function public.cloud_node_heartbeat(text) to anon, authenticated;

-- Existing callers share the bounded path instead of an unthrottled write.
create or replace function public.heartbeat_cloud_device(p_device_token text)
returns void language plpgsql security invoker set search_path = '' as $$
begin
    perform public.cloud_node_heartbeat(p_device_token);
end;
$$;
commit;
