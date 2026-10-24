-- gah_private.resource_permission(workspace, node, resource, user) answers for any user id. It was executable by
-- authenticated for the cloud_nodes_read policy, so any member could probe another member's node permissions, which
-- also reveals their membership and entitlement. Only definer code may ask about someone else; policies and
-- members ask about themselves through the three-argument form.
begin;

revoke all on function gah_private.resource_permission(uuid, text, text, uuid) from public, anon, authenticated;

create or replace function gah_private.resource_permission(p_workspace uuid, p_node text, p_resource text)
returns text language sql stable security definer set search_path = '' as $$
    select gah_private.resource_permission(p_workspace, p_node, p_resource, auth.uid());
$$;
revoke all on function gah_private.resource_permission(uuid, text, text) from public, anon;
grant execute on function gah_private.resource_permission(uuid, text, text) to authenticated;

drop policy if exists cloud_nodes_read on public.nodes;
create policy cloud_nodes_read on public.nodes for select to authenticated using (
    gah_private.node_permission(workspace_id, node_id) is not null
    or gah_private.resource_permission(workspace_id, node_id, 'inference') is not null);

commit;
