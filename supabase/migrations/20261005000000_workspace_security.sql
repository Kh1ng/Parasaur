begin;

-- Deduplicate the unpublished prototype before enforcing one invitation per recipient.
with ranked as (
    select code, row_number() over (partition by workspace_id, user_id order by created_at desc, code) as position
    from gah_private.workspace_invites
) delete from gah_private.workspace_invites where code in (select code from ranked where position > 1);
create unique index if not exists workspace_invite_recipient on gah_private.workspace_invites(workspace_id, user_id);
alter table gah_private.workspace_invites add column if not exists recipient_name text not null default 'Member';

-- Operator-only bootstrap. This establishes a role, never membership or an entitlement.
create or replace function gah_private.bootstrap_workspace_owner(p_workspace uuid, p_user uuid)
returns void language plpgsql security definer set search_path = '' as $$
begin
    perform 1 from public.workspaces where id = p_workspace for update;
    if not found then raise exception 'workspace_not_found' using errcode = '22023'; end if;
    if exists (select 1 from gah_private.memberships where workspace_id = p_workspace and role = 'owner' and user_id <> p_user) then
        raise exception 'workspace_already_has_owner' using errcode = '22023'; end if;
    update gah_private.memberships set role = 'owner' where workspace_id = p_workspace and user_id = p_user;
    if not found then raise exception 'workspace_member_required' using errcode = '22023'; end if;
end;
$$;
revoke all on function gah_private.bootstrap_workspace_owner(uuid, uuid) from public, anon, authenticated;
grant execute on function gah_private.bootstrap_workspace_owner(uuid, uuid) to service_role;

create or replace function public.manage_workspace_members(p_workspace uuid, p_action text, p_email text default null, p_user uuid default null, p_code uuid default null)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare invitation gah_private.workspace_invites; invited_user uuid; recipient_name text; members jsonb; invitations jsonb; can_manage boolean;
begin
    if auth.uid() is null then raise exception 'sign_in_required' using errcode = '42501'; end if;
    if p_action = 'accept' then
        select * into invitation from gah_private.workspace_invites where code = p_code and user_id = auth.uid();
        if not found then raise exception 'invite_invalid_or_expired' using errcode = '22023'; end if;
        -- All invitation/member writes lock the workspace before changing rows.
        perform 1 from public.workspaces where id = invitation.workspace_id for update;
        delete from gah_private.workspace_invites where code = p_code and user_id = auth.uid() and expires_at > statement_timestamp()
            returning * into invitation;
        if not found then raise exception 'invite_invalid_or_expired' using errcode = '22023'; end if;
        if not gah_private.member_active(invitation.workspace_id, invitation.invited_by) or not exists (
            select 1 from gah_private.memberships where workspace_id = invitation.workspace_id and user_id = invitation.invited_by and role = 'owner'
        ) then raise exception 'inviter_access_expired' using errcode = '42501'; end if;
        insert into gah_private.memberships(workspace_id, user_id, display_name, role)
            values (invitation.workspace_id, auth.uid(), invitation.recipient_name, 'member') on conflict do nothing;
        return jsonb_build_object('workspace_id', invitation.workspace_id, 'active', gah_private.can_access(invitation.workspace_id));
    end if;
    if p_action = 'list' then
        if not gah_private.can_access(p_workspace) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
        can_manage := gah_private.workspace_owner(p_workspace);
        select coalesce(jsonb_agg(jsonb_build_object('user_id', user_id, 'display_name', display_name, 'role', role) order by display_name, user_id), '[]')
            into members from gah_private.memberships where workspace_id = p_workspace;
        invitations := '[]';
        if can_manage then
            select coalesce(jsonb_agg(jsonb_build_object('id', code, 'recipient_name', i.recipient_name, 'expires_at', expires_at) order by created_at), '[]')
                into invitations from gah_private.workspace_invites i where workspace_id = p_workspace and expires_at > statement_timestamp();
        end if;
        return jsonb_build_object('members', members, 'can_manage', can_manage, 'invitations', invitations);
    end if;
    if not gah_private.workspace_owner(p_workspace) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    perform 1 from public.workspaces where id = p_workspace for update;
    if not gah_private.workspace_owner(p_workspace) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    if p_action = 'invite' then
        if p_email is null or p_email <> btrim(p_email) or length(p_email) > 254
            or p_email !~ '^[^[:space:]@]+@[^[:space:]@]+\.[^[:space:]@]+$' then
            raise exception 'invalid_email' using errcode = '22023'; end if;
        select id, left(split_part(email, '@', 1), 100) into invited_user, recipient_name from auth.users where lower(email) = lower(p_email);
        if invited_user is null then raise exception 'account_not_found' using errcode = '22023'; end if;
        if exists (select 1 from gah_private.memberships where workspace_id = p_workspace and user_id = invited_user) then
            return jsonb_build_object('invited', false, 'already_member', true);
        end if;
        select * into invitation from gah_private.workspace_invites where workspace_id = p_workspace and user_id = invited_user;
        if found and invitation.expires_at > statement_timestamp() and gah_private.member_active(p_workspace, invitation.invited_by)
            and exists (select 1 from gah_private.memberships where workspace_id = p_workspace and user_id = invitation.invited_by and role = 'owner') then
            return jsonb_build_object('invited', true, 'already_pending', true);
        end if;
        delete from gah_private.workspace_invites where workspace_id = p_workspace and expires_at <= statement_timestamp();
        if (select count(*) from gah_private.workspace_invites where workspace_id = p_workspace) >= 20 then
            raise exception 'invitation_limit' using errcode = '22023'; end if;
        insert into gah_private.workspace_invites(workspace_id, user_id, invited_by, recipient_name)
            values (p_workspace, invited_user, auth.uid(), recipient_name)
            on conflict (workspace_id, user_id) do update set code = gen_random_uuid(), invited_by = excluded.invited_by,
                recipient_name = excluded.recipient_name, created_at = statement_timestamp(), expires_at = statement_timestamp() + interval '24 hours';
        return jsonb_build_object('invited', true, 'already_pending', false);
    elsif p_action = 'remove' then
        if p_user = auth.uid() then raise exception 'cannot_remove_yourself' using errcode = '22023'; end if;
        -- Owner removal is unavailable through this API; even concurrent calls preserve the last owner.
        delete from gah_private.memberships where workspace_id = p_workspace and user_id = p_user and role <> 'owner';
        if not found then raise exception 'member_not_removable' using errcode = '22023'; end if;
        delete from gah_private.workspace_invites where workspace_id = p_workspace and (user_id = p_user or invited_by = p_user);
        update gah_private.dashboard_channels set closed = true where workspace_id = p_workspace and requester = p_user;
        update public.work_items set state = 'cancelled', updated_at = statement_timestamp()
            where workspace_id = p_workspace and requester_id = p_user and state = 'queued';
        return jsonb_build_object('removed', true);
    elsif p_action = 'cancel_invite' then
        delete from gah_private.workspace_invites where workspace_id = p_workspace and code = p_code;
        return jsonb_build_object('cancelled', true);
    end if;
    raise exception 'invalid_action' using errcode = '22023';
end;
$$;
revoke all on function public.manage_workspace_members(uuid, text, text, uuid, uuid) from public, anon;
grant execute on function public.manage_workspace_members(uuid, text, text, uuid, uuid) to authenticated;

-- Dashboard use is an additional consent, not a side effect of lending compute.
alter table public.nodes add column if not exists dashboard_scope text not null default 'private'
    check (dashboard_scope in ('private', 'workspace', 'selected'));
create table if not exists gah_private.dashboard_grants (
    workspace_id uuid not null, node_id text not null, user_id uuid not null,
    primary key (workspace_id, node_id, user_id),
    foreign key (workspace_id, node_id) references public.nodes on delete cascade,
    foreign key (workspace_id, user_id) references gah_private.memberships on delete cascade
);
alter table gah_private.dashboard_grants enable row level security;
revoke all on gah_private.dashboard_grants from public, anon, authenticated;
grant all on gah_private.dashboard_grants to service_role;
create or replace function gah_private.dashboard_permission(p_workspace uuid, p_node text, p_user uuid)
returns text language sql stable security definer set search_path = '' as $$
    select case when n.owner_id = p_user then 'owner'
        when n.dashboard_scope = 'workspace' then 'use'
        when n.dashboard_scope = 'selected' and exists (select 1 from gah_private.dashboard_grants g
            where g.workspace_id = n.workspace_id and g.node_id = n.node_id and g.user_id = p_user) then 'use' end
    from public.nodes n where n.workspace_id = p_workspace and n.node_id = p_node and n.revoked_at is null
        and gah_private.resource_permission(p_workspace, p_node, 'compute', p_user) is not null
        and gah_private.resource_permission(p_workspace, p_node, 'inference', p_user) is not null;
$$;
revoke all on function gah_private.dashboard_permission(uuid, text, uuid) from public, anon, authenticated;
create or replace function public.configure_cloud_dashboard(p_workspace_id uuid, p_node_id text, p_scope text)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if p_scope is null or p_scope not in ('private', 'workspace', 'selected') then raise exception 'invalid_dashboard_scope' using errcode = '22023'; end if;
    update public.nodes set dashboard_scope = p_scope where workspace_id = p_workspace_id and node_id = p_node_id and revoked_at is null
        and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
    if not found then raise exception 'node_write_denied' using errcode = '42501'; end if;
end;
$$;
create or replace function public.set_cloud_dashboard_grant(p_workspace_id uuid, p_node_id text, p_user_id uuid, p_permission text)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if gah_private.node_permission(p_workspace_id, p_node_id) is distinct from 'owner' then raise exception 'node_write_denied' using errcode = '42501'; end if;
    if p_permission is null or p_permission not in ('use', 'remove') then raise exception 'invalid_dashboard_permission' using errcode = '22023'; end if;
    if p_permission = 'remove' then
        delete from gah_private.dashboard_grants where workspace_id = p_workspace_id and node_id = p_node_id and user_id = p_user_id;
    else
        if not exists (select 1 from gah_private.memberships where workspace_id = p_workspace_id and user_id = p_user_id) then
            raise exception 'workspace_member_required' using errcode = '42501'; end if;
        insert into gah_private.dashboard_grants values (p_workspace_id, p_node_id, p_user_id) on conflict do nothing;
    end if;
end;
$$;
create or replace function public.cloud_dashboard_permissions(p_workspace_id uuid)
returns table(node_id text, permission text) language sql stable security definer set search_path = '' as $$
    select n.node_id, gah_private.dashboard_permission(n.workspace_id, n.node_id, auth.uid()) from public.nodes n
    where n.workspace_id = p_workspace_id and gah_private.dashboard_permission(n.workspace_id, n.node_id, auth.uid()) is not null;
$$;
create or replace function public.cloud_dashboard_grants(p_workspace_id uuid, p_node_id text)
returns table(user_id uuid) language sql stable security definer set search_path = '' as $$
    select g.user_id from gah_private.dashboard_grants g where g.workspace_id = p_workspace_id and g.node_id = p_node_id
        and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
$$;
revoke all on function public.configure_cloud_dashboard(uuid, text, text), public.set_cloud_dashboard_grant(uuid, text, uuid, text),
    public.cloud_dashboard_permissions(uuid), public.cloud_dashboard_grants(uuid, text) from public, anon;
grant execute on function public.configure_cloud_dashboard(uuid, text, text), public.set_cloud_dashboard_grant(uuid, text, uuid, text),
    public.cloud_dashboard_permissions(uuid), public.cloud_dashboard_grants(uuid, text) to authenticated;

-- Native execution would use the compute host's own login. It requires a separate
-- inference grant. An explicit remote inference binding uses that resource's grant.
create or replace function gah_private.job_allowed(w public.work_items)
returns boolean language sql stable security definer set search_path = '' as $$
    select exists (select 1 from public.nodes n where n.workspace_id = w.workspace_id and n.node_id = w.node_id
        and n.revoked_at is null and w.profile = any(n.profiles) and w.memory_mb <= n.memory_budget_mb)
    and gah_private.resource_permission(w.workspace_id, w.node_id, 'compute', w.requester_id) is not null
    and (w.device_id is null or exists (select 1 from gah_private.devices d where d.id = w.device_id and d.revoked_at is null))
    and case when w.inference_node_id is null then
        gah_private.resource_permission(w.workspace_id, w.node_id, 'inference', w.requester_id) is not null
    else exists (select 1 from public.nodes i where i.workspace_id = w.workspace_id and i.node_id = w.inference_node_id
        and i.revoked_at is null and w.inference_model = any(i.inference_models)
        and gah_private.resource_permission(w.workspace_id, w.inference_node_id, 'inference', w.requester_id) is not null) end;
$$;
revoke all on function gah_private.job_allowed(public.work_items) from public, anon, authenticated;

-- Awareness exposes counts only, including when no machine or API is shared.
create or replace function public.cloud_workspace_activity(p_workspace_id uuid)
returns table(user_id uuid, display_name text, active_jobs bigint)
language sql stable security definer set search_path = '' as $$
    select m.user_id, m.display_name, (select count(*) from public.work_items w
        where w.workspace_id = m.workspace_id and w.requester_id = m.user_id and w.state in ('queued', 'running'))
    from gah_private.memberships m where m.workspace_id = p_workspace_id and gah_private.can_access(p_workspace_id);
$$;
create or replace function public.configure_cloud_sharing(p_workspace_id uuid, p_node_id text, p_mode text)
returns void language plpgsql security definer set search_path = '' as $$
begin
    if p_mode is null or p_mode not in ('none', 'compute', 'all') then raise exception 'invalid_sharing_mode' using errcode = '22023'; end if;
    update public.nodes set sharing_scope = case when p_mode = 'none' then 'private' else 'workspace' end,
        inference_scope = case when p_mode = 'all' then 'workspace' else 'private' end,
        dashboard_scope = case when p_mode = 'all' then 'workspace' else 'private' end
    where workspace_id = p_workspace_id and node_id = p_node_id and revoked_at is null
        and gah_private.node_permission(p_workspace_id, p_node_id) = 'owner';
    if not found then raise exception 'node_write_denied' using errcode = '42501'; end if;
end;
$$;
revoke all on function public.cloud_workspace_activity(uuid), public.configure_cloud_sharing(uuid, text, text) from public, anon;
grant execute on function public.cloud_workspace_activity(uuid), public.configure_cloud_sharing(uuid, text, text) to authenticated;

-- Recheck the additional permission on opens, claims, sends, receives, and device replies.
create or replace function public.cloud_dashboard(p_action text, p_body jsonb default '{}', p_device_token text default null)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices; channel gah_private.dashboard_channels; result jsonb; permission text;
begin
    if p_body is null or octet_length(p_body::text) > 1048576 then raise exception 'invalid_dashboard_request' using errcode = '22023'; end if;
    if p_action in ('claim', 'post', 'check') then
        d := gah_private.device(p_device_token);
        if p_action = 'claim' then
            update public.nodes set dashboard_heartbeat_at = statement_timestamp()
                where workspace_id = d.workspace_id and node_id = d.node_id;
            delete from gah_private.dashboard_channels where expires_at < statement_timestamp() - interval '5 minutes';
            with pending as (
                select q.id from gah_private.dashboard_commands q join gah_private.dashboard_channels c on c.id = q.channel_id
                where c.workspace_id = d.workspace_id and c.node_id = d.node_id and not q.claimed and not c.closed
                    and c.expires_at > statement_timestamp() and (c.device_id is null or c.device_id = d.id)
                    and gah_private.dashboard_permission(c.workspace_id, c.node_id, c.requester) is not null
                order by q.created_at for update of q skip locked limit 32
            ), claimed as (
                update gah_private.dashboard_commands q set claimed = true where q.id in (select id from pending) returning q.*
            ) select coalesce(jsonb_agg(jsonb_build_object('id', q.id, 'channel_id', q.channel_id, 'action', q.action, 'payload', q.payload,
                'user_id', c.requester, 'permission', gah_private.dashboard_permission(c.workspace_id, c.node_id, c.requester))), '[]')
                into result from claimed q join gah_private.dashboard_channels c on c.id = q.channel_id;
            update gah_private.dashboard_channels c set device_id = d.id where c.workspace_id = d.workspace_id and c.node_id = d.node_id
                and c.id in (select (x->>'channel_id')::uuid from jsonb_array_elements(result) x);
            return result;
        end if;
        select * into channel from gah_private.dashboard_channels where id = (p_body->>'channel_id')::uuid;
        if not found or channel.workspace_id <> d.workspace_id or channel.node_id <> d.node_id or channel.device_id is distinct from d.id
            or channel.closed or channel.expires_at <= statement_timestamp()
            or gah_private.dashboard_permission(channel.workspace_id, channel.node_id, channel.requester) is null then return 'false'; end if;
        if p_action = 'check' then return to_jsonb(true); end if;
        if octet_length((p_body->'message')::text) > 262144 then raise exception 'dashboard_message_too_large' using errcode = '22023'; end if;
        if (select count(*) from gah_private.dashboard_messages where channel_id = channel.id) >= 256 then
            raise exception 'dashboard_backpressure' using errcode = '22023'; end if;
        insert into gah_private.dashboard_messages(channel_id, payload) values (channel.id, p_body->'message');
        return to_jsonb(true);
    end if;
    if auth.uid() is null then raise exception 'sign_in_required' using errcode = '42501'; end if;
    if p_action in ('http', 'open') then
        permission := gah_private.dashboard_permission((p_body->>'workspace_id')::uuid, p_body->>'node_id', auth.uid());
        if permission is null then raise exception 'dashboard_access_denied' using errcode = '42501'; end if;
        if not exists (select 1 from public.nodes where workspace_id = (p_body->>'workspace_id')::uuid and node_id = p_body->>'node_id'
            and dashboard_heartbeat_at > statement_timestamp() - interval '15 seconds' and revoked_at is null) then
            raise exception 'dashboard_offline' using errcode = '22023'; end if;
        if (select count(*) from gah_private.dashboard_channels where requester = auth.uid() and not closed and expires_at > statement_timestamp()) >= 64
            then raise exception 'dashboard_channel_limit' using errcode = '22023'; end if;
        insert into gah_private.dashboard_channels(id, workspace_id, node_id, requester, kind)
            values ((p_body->>'channel_id')::uuid, (p_body->>'workspace_id')::uuid, p_body->>'node_id', auth.uid(), case p_action when 'http' then 'http' else 'ws' end);
    else
        select * into channel from gah_private.dashboard_channels where id = (p_body->>'channel_id')::uuid and requester = auth.uid();
        if not found or channel.closed or channel.expires_at <= statement_timestamp()
            or gah_private.dashboard_permission(channel.workspace_id, channel.node_id, auth.uid()) is null then
            raise exception 'dashboard_access_denied' using errcode = '42501'; end if;
    end if;
    if p_action = 'receive' then
        update gah_private.dashboard_channels set expires_at = statement_timestamp() + interval '30 seconds' where id = channel.id;
        with selected as (select id from gah_private.dashboard_messages where channel_id = channel.id order by id limit 64),
            removed as (delete from gah_private.dashboard_messages where id in (select id from selected) returning *)
            select coalesce(jsonb_agg(payload order by id), '[]') into result from removed;
        return result;
    elsif p_action = 'close' then
        update gah_private.dashboard_channels set closed = true where id = channel.id;
        return to_jsonb(true);
    elsif p_action not in ('http', 'open', 'send') then raise exception 'invalid_dashboard_action' using errcode = '22023'; end if;
    insert into gah_private.dashboard_commands(id, channel_id, action, payload)
        values ((p_body->>'request_id')::uuid, (p_body->>'channel_id')::uuid, p_action, coalesce(p_body->'payload', '{}'));
    return to_jsonb(true);
end;
$$;
revoke all on function public.cloud_dashboard(text, jsonb, text) from public;
-- Device-only actions authenticate their bounded node credential inside the function.
grant execute on function public.cloud_dashboard(text, jsonb, text) to anon, authenticated;

commit;
