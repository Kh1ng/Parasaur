begin;
alter table public.nodes add column if not exists dashboard_heartbeat_at timestamptz;
create table if not exists gah_private.dashboard_channels (
    id uuid primary key, workspace_id uuid not null, node_id text not null, requester uuid not null references auth.users,
    kind text not null check (kind in ('http', 'ws')), device_id uuid references gah_private.devices,
    expires_at timestamptz not null default statement_timestamp() + interval '30 seconds', closed boolean not null default false,
    foreign key(workspace_id, node_id) references public.nodes
);
create table if not exists gah_private.dashboard_commands (
    id uuid primary key, channel_id uuid not null references gah_private.dashboard_channels on delete cascade,
    action text not null check (action in ('http', 'open', 'send', 'close')), payload jsonb not null,
    claimed boolean not null default false, created_at timestamptz not null default statement_timestamp()
);
create table if not exists gah_private.dashboard_messages (
    id bigint generated always as identity primary key, channel_id uuid not null references gah_private.dashboard_channels on delete cascade,
    payload jsonb not null, created_at timestamptz not null default statement_timestamp()
);
alter table gah_private.dashboard_channels enable row level security;
alter table gah_private.dashboard_commands enable row level security;
alter table gah_private.dashboard_messages enable row level security;
revoke all on gah_private.dashboard_channels, gah_private.dashboard_commands, gah_private.dashboard_messages from public, anon, authenticated;

-- Polling is bounded; every command and response rechecks live resource authority.
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
                    and gah_private.resource_permission(c.workspace_id, c.node_id, 'compute', c.requester) is not null
                order by q.created_at for update of q skip locked limit 32
            ), claimed as (
                update gah_private.dashboard_commands q set claimed = true where q.id in (select id from pending) returning q.*
            ) select coalesce(jsonb_agg(jsonb_build_object('id', q.id, 'channel_id', q.channel_id, 'action', q.action, 'payload', q.payload,
                'user_id', c.requester, 'permission', gah_private.resource_permission(c.workspace_id, c.node_id, 'compute', c.requester))), '[]')
                into result from claimed q join gah_private.dashboard_channels c on c.id = q.channel_id;
            update gah_private.dashboard_channels c set device_id = d.id where c.workspace_id = d.workspace_id and c.node_id = d.node_id
                and c.id in (select (x->>'channel_id')::uuid from jsonb_array_elements(result) x);
            return result;
        end if;
        select * into channel from gah_private.dashboard_channels where id = (p_body->>'channel_id')::uuid;
        if not found or channel.workspace_id <> d.workspace_id or channel.node_id <> d.node_id or channel.device_id is distinct from d.id
            or channel.closed or channel.expires_at <= statement_timestamp()
            or gah_private.resource_permission(channel.workspace_id, channel.node_id, 'compute', channel.requester) is null then return 'false'; end if;
        if p_action = 'check' then return to_jsonb(true); end if;
        if octet_length((p_body->'message')::text) > 262144 then raise exception 'dashboard_message_too_large' using errcode = '22023'; end if;
        if (select count(*) from gah_private.dashboard_messages where channel_id = channel.id) >= 256 then
            raise exception 'dashboard_backpressure' using errcode = '22023'; end if;
        insert into gah_private.dashboard_messages(channel_id, payload) values (channel.id, p_body->'message');
        return to_jsonb(true);
    end if;
    if auth.uid() is null then raise exception 'sign_in_required' using errcode = '42501'; end if;
    if p_action in ('http', 'open') then
        permission := gah_private.resource_permission((p_body->>'workspace_id')::uuid, p_body->>'node_id', 'compute', auth.uid());
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
            or gah_private.resource_permission(channel.workspace_id, channel.node_id, 'compute', auth.uid()) is null then
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
