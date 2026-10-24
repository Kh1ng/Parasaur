begin;

-- Existing pilot memberships stay members until the operator explicitly assigns an owner.
alter table gah_private.memberships add column if not exists role text not null default 'member'
    check (role in ('owner', 'member'));
create table if not exists gah_private.workspace_invites (
    code uuid primary key default gen_random_uuid(), workspace_id uuid not null references public.workspaces on delete cascade,
    user_id uuid not null references auth.users, invited_by uuid not null references auth.users,
    created_at timestamptz not null default statement_timestamp(),
    expires_at timestamptz not null default statement_timestamp() + interval '24 hours'
);
alter table gah_private.workspace_invites enable row level security;
revoke all on gah_private.workspace_invites from public, anon, authenticated;
grant all on gah_private.workspace_invites to service_role;

create or replace function gah_private.workspace_owner(p_workspace uuid) returns boolean
language sql stable security definer set search_path = '' as $$
    select gah_private.can_access(p_workspace) and exists (
        select 1 from gah_private.memberships where workspace_id = p_workspace and user_id = auth.uid() and role = 'owner');
$$;
revoke all on function gah_private.workspace_owner(uuid) from public, anon;
grant execute on function gah_private.workspace_owner(uuid) to authenticated;

-- Caller JWT and live owner membership remain authoritative even behind an Edge Function.
create or replace function public.manage_workspace_members(p_workspace uuid, p_action text, p_email text default null, p_user uuid default null, p_code uuid default null)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare invitation gah_private.workspace_invites; invited_user uuid; response jsonb;
begin
    if auth.uid() is null then raise exception 'sign_in_required' using errcode = '42501'; end if;
    if p_action = 'accept' then
        -- Row deletion serializes redemption; a code is email-bound and works once.
        delete from gah_private.workspace_invites where code = p_code and expires_at > statement_timestamp()
            and user_id = auth.uid() returning * into invitation;
        if not found then raise exception 'invite_invalid_or_expired' using errcode = '22023'; end if;
        if not gah_private.member_active(invitation.workspace_id, invitation.invited_by) or not exists (
            select 1 from gah_private.memberships where workspace_id = invitation.workspace_id and user_id = invitation.invited_by and role = 'owner'
        ) then raise exception 'inviter_access_expired' using errcode = '42501'; end if;
        insert into gah_private.memberships(workspace_id, user_id, display_name, role)
            values (invitation.workspace_id, auth.uid(), 'Member', 'member') on conflict do nothing;
        return jsonb_build_object('workspace_id', invitation.workspace_id, 'active', gah_private.can_access(invitation.workspace_id));
    end if;
    if p_action = 'list' then
        if not gah_private.can_access(p_workspace) then raise exception 'workspace_access_denied' using errcode = '42501'; end if;
        select coalesce(jsonb_agg(jsonb_build_object('user_id', user_id, 'display_name', display_name, 'role', role)), '[]') into response
            from gah_private.memberships where workspace_id = p_workspace;
        return jsonb_build_object('members', response, 'can_manage', gah_private.workspace_owner(p_workspace));
    end if;
    if not gah_private.workspace_owner(p_workspace) then raise exception 'workspace_owner_required' using errcode = '42501'; end if;
    -- Membership mutations lock the workspace, so concurrent removals cannot remove its last owner.
    perform 1 from public.workspaces where id = p_workspace for update;
    if p_action = 'invite' then
        if p_email is null or p_email <> btrim(p_email) or length(p_email) > 254
            or p_email !~ '^[^[:space:]@]+@[^[:space:]@]+\.[^[:space:]@]+$' then
            raise exception 'invalid_email' using errcode = '22023'; end if;
        if (select count(*) from gah_private.workspace_invites where workspace_id = p_workspace and created_at > statement_timestamp() - interval '1 hour') >= 20
            then raise exception 'invitation_limit' using errcode = '22023'; end if;
        select id into invited_user from auth.users where lower(email) = lower(p_email);
        if invited_user is null then raise exception 'account_not_found' using errcode = '22023'; end if;
        insert into gah_private.workspace_invites(workspace_id, user_id, invited_by)
            values (p_workspace, invited_user, auth.uid()) returning * into invitation;
        return jsonb_build_object('invited', true);
    elsif p_action = 'remove' then
        if p_user = auth.uid() then raise exception 'cannot_remove_yourself' using errcode = '22023'; end if;
        delete from gah_private.memberships where workspace_id = p_workspace and user_id = p_user and role <> 'owner';
        if not found then raise exception 'member_not_removable' using errcode = '22023'; end if;
        delete from gah_private.workspace_invites where workspace_id = p_workspace and invited_by = p_user;
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

-- Registration establishes identity. Only an authenticated worker establishes liveness.
alter table public.nodes add column if not exists worker_heartbeat_at timestamptz;
create or replace function public.heartbeat_cloud_device(p_device_token text)
returns void language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices := gah_private.device(p_device_token);
begin
    update public.nodes set last_seen_at = statement_timestamp(), worker_heartbeat_at = statement_timestamp()
        where workspace_id = d.workspace_id and node_id = d.node_id;
end;
$$;
create or replace function public.cloud_node_health(p_workspace uuid)
returns table(node_id text, worker_heartbeat_at timestamptz, state text)
language sql stable security invoker set search_path = '' as $$
    select node_id, worker_heartbeat_at, case when revoked_at is not null then 'revoked'
        when worker_heartbeat_at is null then 'unknown'
        when worker_heartbeat_at > statement_timestamp() - interval '90 seconds' then 'online' else 'offline' end
    from public.nodes where workspace_id = p_workspace;
$$;
revoke all on function public.cloud_node_health(uuid) from public, anon;
grant execute on function public.cloud_node_health(uuid) to authenticated;
create or replace function public.pending_workspace_invites()
returns table(id uuid, workspace_id uuid, workspace_name text, inviter_name text)
language sql stable security definer set search_path = '' as $$
    select i.code, i.workspace_id, w.name, m.display_name
    from gah_private.workspace_invites i join public.workspaces w on w.id = i.workspace_id
    join gah_private.memberships m on m.workspace_id = i.workspace_id and m.user_id = i.invited_by
    where i.user_id = auth.uid() and i.expires_at > statement_timestamp() and m.role = 'owner'
      and gah_private.member_active(i.workspace_id, i.invited_by);
$$;
revoke all on function public.pending_workspace_invites() from public, anon;
grant execute on function public.pending_workspace_invites() to authenticated;
commit;
