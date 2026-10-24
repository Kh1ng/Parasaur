begin;
alter table gah_private.dashboard_channels add column if not exists last_delivered bigint not null default 0;
alter table gah_private.dashboard_commands add column if not exists sequence bigint generated always as identity;
alter table gah_private.dashboard_commands add column if not exists claimed_at timestamptz;
alter table gah_private.dashboard_commands add column if not exists completed boolean not null default false;
alter table gah_private.dashboard_commands add column if not exists completed_at timestamptz;
alter table gah_private.dashboard_commands add column if not exists payload_hash text;
update gah_private.dashboard_commands set payload_hash = encode(sha256(convert_to(payload::text, 'UTF8')), 'hex') where payload_hash is null;
update gah_private.dashboard_commands set completed_at = statement_timestamp() where completed and completed_at is null;
alter table gah_private.dashboard_messages add column if not exists delivery_id uuid not null default gen_random_uuid();
alter table gah_private.dashboard_messages add column if not exists acknowledged boolean not null default false;
alter table gah_private.dashboard_messages add column if not exists acknowledged_at timestamptz;
update gah_private.dashboard_messages set acknowledged_at = statement_timestamp() where acknowledged and acknowledged_at is null;
alter table gah_private.dashboard_messages add column if not exists payload_hash text;
update gah_private.dashboard_messages set payload_hash = encode(sha256(convert_to(payload::text, 'UTF8')), 'hex') where payload_hash is null;
create unique index if not exists dashboard_delivery_identity on gah_private.dashboard_messages(channel_id, delivery_id);
update gah_private.dashboard_commands set claimed_at = statement_timestamp() where claimed and claimed_at is null;

-- Retry receipts expire after 30 seconds; clients stop automatic retries after 25 seconds.
create index if not exists dashboard_pending_commands on gah_private.dashboard_commands(sequence, channel_id) where not claimed;
create index if not exists dashboard_inflight_commands on gah_private.dashboard_commands(channel_id, claimed_at) where not completed;
create index if not exists dashboard_pending_messages on gah_private.dashboard_messages(channel_id, id) where not acknowledged;
create index if not exists dashboard_completed_receipts on gah_private.dashboard_commands(channel_id, completed_at) where completed;
create index if not exists dashboard_acknowledged_receipts on gah_private.dashboard_messages(channel_id, acknowledged_at) where acknowledged;
create or replace function gah_private.prune_dashboard_receipts(p_channel uuid)
returns void language sql set search_path = '' as $$
    delete from gah_private.dashboard_commands where channel_id = p_channel and completed
        and completed_at < statement_timestamp() - interval '30 seconds';
    delete from gah_private.dashboard_messages where channel_id = p_channel and acknowledged
        and acknowledged_at < statement_timestamp() - interval '30 seconds';
$$;
revoke all on function gah_private.prune_dashboard_receipts(uuid) from public, anon, authenticated;

-- Reject credential transfer before persistence, including direct RPC callers.
create or replace function gah_private.cloud_request_body_safe(p_text text)
returns boolean language plpgsql immutable set search_path = '' as $$
begin
    if p_text is null or p_text = '' then return true; end if;
    return not jsonb_path_exists(p_text::jsonb, '$.** ? (@.type() == "object").keyvalue() ? (@.key like_regex "^(api[_-]?key|password|authorization|cookie|token|device[_-]?token|access[_-]?token|refresh[_-]?token|coordinator[_-]?token)$" flag "i")');
exception when invalid_text_representation then return false;
end;
$$;
revoke all on function gah_private.cloud_request_body_safe(text) from public, anon, authenticated;

-- Frames remain durable until the recipient acknowledges them. Commands never replay
-- after an uncertain local execution; a lost connector lease produces an explicit error.
create or replace function public.cloud_dashboard(p_action text, p_body jsonb default '{}', p_device_token text default null)
returns jsonb language plpgsql security definer set search_path = '' as $$
declare d gah_private.devices; channel gah_private.dashboard_channels; result jsonb; permission text; ack bigint; existing gah_private.dashboard_commands; delivery uuid;
begin
    if p_body is null or octet_length(p_body::text) > 1048576 then raise exception 'invalid_dashboard_request' using errcode = '22023'; end if;
    if p_action in ('claim', 'post', 'check', 'check_many', 'complete') then
        d := gah_private.device(p_device_token);
        if p_action = 'check_many' then
            if jsonb_typeof(p_body->'channel_ids') is distinct from 'array' or jsonb_array_length(p_body->'channel_ids') > 64 then
                raise exception 'invalid_dashboard_request' using errcode = '22023'; end if;
            with allowed as (
                select c.id from gah_private.dashboard_channels c where c.id in (select value::uuid from jsonb_array_elements_text(p_body->'channel_ids'))
                    and c.workspace_id = d.workspace_id and c.node_id = d.node_id and c.device_id = d.id and not c.closed
                    and c.expires_at > statement_timestamp() and gah_private.dashboard_permission(c.workspace_id, c.node_id, c.requester) is not null
            ), renewed as (
                update gah_private.dashboard_commands set claimed_at = statement_timestamp() where channel_id in (select id from allowed)
                    and claimed and not completed returning id
            ) select coalesce(jsonb_agg(id), '[]') into result from allowed;
            return result;
        end if;
        if p_action = 'claim' then
            update public.nodes set dashboard_heartbeat_at = statement_timestamp()
                where workspace_id = d.workspace_id and node_id = d.node_id;
            delete from gah_private.dashboard_channels where expires_at < statement_timestamp() - interval '5 minutes';
            -- Lost execution leases fail explicitly. Never replay an uncertain local mutation.
            insert into gah_private.dashboard_messages(channel_id, payload, payload_hash)
                select distinct on (q.channel_id) q.channel_id, jsonb_build_object('type', 'error', 'reason', 'connector_interrupted'),
                    encode(sha256(convert_to(jsonb_build_object('type', 'error', 'reason', 'connector_interrupted')::text, 'UTF8')), 'hex')
                from gah_private.dashboard_commands q join gah_private.dashboard_channels c on c.id = q.channel_id
                where c.workspace_id = d.workspace_id and c.node_id = d.node_id and q.claimed and not q.completed
                    and q.claimed_at < statement_timestamp() - interval '30 seconds' and not c.closed
                order by q.channel_id, q.created_at, q.id;
            update gah_private.dashboard_commands q set completed = true, completed_at = statement_timestamp(), payload = 'null'::jsonb
                from gah_private.dashboard_channels c where c.id = q.channel_id and c.workspace_id = d.workspace_id and c.node_id = d.node_id
                    and q.claimed and not q.completed and q.claimed_at < statement_timestamp() - interval '30 seconds';
            with pending as (
                select q.id from gah_private.dashboard_commands q join gah_private.dashboard_channels c on c.id = q.channel_id
                where c.workspace_id = d.workspace_id and c.node_id = d.node_id and not q.claimed and not c.closed
                    and c.expires_at > statement_timestamp() and (c.device_id is null or c.device_id = d.id)
                    and gah_private.dashboard_permission(c.workspace_id, c.node_id, c.requester) is not null
                order by q.sequence for update of q skip locked limit 32
            ), claimed as (
                update gah_private.dashboard_commands q set claimed = true, claimed_at = statement_timestamp() where q.id in (select id from pending) returning q.*
            ) select coalesce(jsonb_agg(jsonb_build_object('id', q.id, 'channel_id', q.channel_id, 'action', q.action, 'payload', q.payload,
                'user_id', c.requester, 'permission', gah_private.dashboard_permission(c.workspace_id, c.node_id, c.requester)) order by q.sequence), '[]')
                into result from claimed q join gah_private.dashboard_channels c on c.id = q.channel_id;
            update gah_private.dashboard_channels c set device_id = d.id where c.workspace_id = d.workspace_id and c.node_id = d.node_id
                and c.id in (select (x->>'channel_id')::uuid from jsonb_array_elements(result) x);
            return result;
        end if;
        select * into channel from gah_private.dashboard_channels where id = (p_body->>'channel_id')::uuid for update;
        if not found or channel.workspace_id <> d.workspace_id or channel.node_id <> d.node_id or channel.device_id is distinct from d.id
            or channel.closed or channel.expires_at <= statement_timestamp()
            or gah_private.dashboard_permission(channel.workspace_id, channel.node_id, channel.requester) is null then return 'false'; end if;
        perform gah_private.prune_dashboard_receipts(channel.id);
        if p_action = 'check' then
            update gah_private.dashboard_commands set claimed_at = statement_timestamp() where channel_id = channel.id and claimed and not completed;
            return to_jsonb(true);
        end if;
        if p_action = 'complete' then
            update gah_private.dashboard_commands set completed = true, completed_at = coalesce(completed_at, statement_timestamp()), payload = 'null'::jsonb where id = (p_body->>'request_id')::uuid and channel_id = channel.id and claimed;
            return to_jsonb(found);
        end if;
        if p_body->'message' is null or jsonb_typeof(p_body->'message') <> 'object' then raise exception 'invalid_dashboard_message' using errcode = '22023'; end if;
        delivery := coalesce((p_body->>'message_id')::uuid, gen_random_uuid());
        if exists (select 1 from gah_private.dashboard_messages where channel_id = channel.id and delivery_id = delivery) then
            if not exists (select 1 from gah_private.dashboard_messages where channel_id = channel.id and delivery_id = delivery and payload_hash = encode(sha256(convert_to((p_body->'message')::text, 'UTF8')), 'hex')) then
                raise exception 'dashboard_delivery_conflict' using errcode = '22023'; end if;
            return to_jsonb(true);
        end if;
        if octet_length((p_body->'message')::text) > 262144 then raise exception 'dashboard_message_too_large' using errcode = '22023'; end if;
        if (select count(*) from gah_private.dashboard_messages where channel_id = channel.id and not acknowledged) >= 256
            or coalesce((select sum(octet_length(payload::text)) from gah_private.dashboard_messages where channel_id = channel.id and not acknowledged), 0) + octet_length((p_body->'message')::text) > 8388608 then
            raise exception 'dashboard_backpressure' using errcode = '22023'; end if;
        insert into gah_private.dashboard_messages(channel_id, delivery_id, payload, payload_hash)
            values (channel.id, delivery, p_body->'message', encode(sha256(convert_to((p_body->'message')::text, 'UTF8')), 'hex'));
        return to_jsonb(true);
    end if;
    if auth.uid() is null then raise exception 'sign_in_required' using errcode = '42501'; end if;
    if p_action = 'http' then
        if (p_body->'payload'->>'path') is null or p_body->'payload'->>'path' !~ '^/api/'
            or p_body->'payload'->>'path' ~* '^/api/(login-repair|auth-health/repairs|settings/gateway/bootstrap-command|devices|pairing)(/|[?]|$)'
            or not gah_private.cloud_request_body_safe(p_body->'payload'->>'body') then
            raise exception 'cloud_credential_transfer_denied' using errcode = '42501'; end if;
        p_body := jsonb_set(p_body, '{payload,headers}', coalesce((select jsonb_object_agg(lower(key), value)
            from jsonb_each(coalesce(p_body->'payload'->'headers', '{}'))
            where lower(key) in ('content-type', 'x-request-id', 'idempotency-key') and jsonb_typeof(value) = 'string' and length(value #>> '{}') <= 512), '{}'));
    end if;
    if p_action in ('http', 'open', 'send') then
        perform pg_advisory_xact_lock(hashtextextended(auth.uid()::text, 0));
        select q.* into existing from gah_private.dashboard_commands q join gah_private.dashboard_channels c on c.id = q.channel_id
            where q.id = (p_body->>'request_id')::uuid and c.requester = auth.uid();
        if found then
            select * into channel from gah_private.dashboard_channels where id = existing.channel_id;
            if channel.closed or channel.expires_at <= statement_timestamp() or gah_private.dashboard_permission(channel.workspace_id, channel.node_id, auth.uid()) is null then
                raise exception 'dashboard_access_denied' using errcode = '42501'; end if;
            if existing.channel_id <> (p_body->>'channel_id')::uuid or existing.action <> p_action or existing.payload_hash <> encode(sha256(convert_to(coalesce(p_body->'payload', '{}')::text, 'UTF8')), 'hex')
                or (p_action in ('http', 'open') and (channel.workspace_id <> (p_body->>'workspace_id')::uuid or channel.node_id <> p_body->>'node_id')) then
                raise exception 'dashboard_request_conflict' using errcode = '22023'; end if;
            return to_jsonb(true);
        end if;
    end if;
    if p_action in ('http', 'open') then
        permission := gah_private.dashboard_permission((p_body->>'workspace_id')::uuid, p_body->>'node_id', auth.uid());
        if permission is null then raise exception 'dashboard_access_denied' using errcode = '42501'; end if;
        if not exists (select 1 from public.nodes where workspace_id = (p_body->>'workspace_id')::uuid and node_id = p_body->>'node_id'
            and dashboard_heartbeat_at > statement_timestamp() - interval '15 seconds' and revoked_at is null) then
            raise exception 'dashboard_offline' using errcode = '22023'; end if;
        perform 1 from public.nodes where workspace_id = (p_body->>'workspace_id')::uuid and node_id = p_body->>'node_id' for update;
        if (select count(*) from gah_private.dashboard_channels where workspace_id = (p_body->>'workspace_id')::uuid and node_id = p_body->>'node_id'
            and not closed and expires_at > statement_timestamp()) >= 128 then raise exception 'dashboard_node_limit' using errcode = '22023'; end if;
        if (select count(*) from gah_private.dashboard_channels where requester = auth.uid() and not closed and expires_at > statement_timestamp()) >= 64
            then raise exception 'dashboard_channel_limit' using errcode = '22023'; end if;
        insert into gah_private.dashboard_channels(id, workspace_id, node_id, requester, kind)
            values ((p_body->>'channel_id')::uuid, (p_body->>'workspace_id')::uuid, p_body->>'node_id', auth.uid(), case p_action when 'http' then 'http' else 'ws' end);
    else
        select * into channel from gah_private.dashboard_channels where id = (p_body->>'channel_id')::uuid and requester = auth.uid() for update;
        if not found or channel.closed or channel.expires_at <= statement_timestamp()
            or gah_private.dashboard_permission(channel.workspace_id, channel.node_id, auth.uid()) is null then
            raise exception 'dashboard_access_denied' using errcode = '42501'; end if;
    end if;
    perform gah_private.prune_dashboard_receipts(channel.id);
    if p_action = 'receive' then
        update gah_private.dashboard_channels set expires_at = statement_timestamp() + interval '30 seconds' where id = channel.id;
        ack := coalesce((p_body->>'ack')::bigint, 0);
        if ack < 0 or ack > channel.last_delivered then raise exception 'invalid_dashboard_ack' using errcode = '22023'; end if;
        update gah_private.dashboard_messages set acknowledged = true, acknowledged_at = statement_timestamp(), payload = 'null'::jsonb where channel_id = channel.id and id <= ack and not acknowledged;
        with selected as (select id, payload from gah_private.dashboard_messages where channel_id = channel.id and not acknowledged order by id limit 64)
            select coalesce(jsonb_agg(jsonb_build_object('id', id::text, 'payload', payload) order by id), '[]') into result from selected;
        update gah_private.dashboard_channels set last_delivered = greatest(last_delivered,
            coalesce((select max((packet->>'id')::bigint) from jsonb_array_elements(result) packet), 0)) where id = channel.id;
        return result;
    elsif p_action = 'close' then
        update gah_private.dashboard_channels set closed = true where id = channel.id;
        return to_jsonb(true);
    elsif p_action not in ('http', 'open', 'send') then raise exception 'invalid_dashboard_action' using errcode = '22023'; end if;
    if (select count(*) from gah_private.dashboard_commands where channel_id = (p_body->>'channel_id')::uuid and not completed) >= 64 then
        raise exception 'dashboard_command_limit' using errcode = '22023'; end if;
    if octet_length(coalesce((p_body->'payload')::text, '{}')) > 262144 then raise exception 'dashboard_message_too_large' using errcode = '22023'; end if;
    if coalesce((select sum(octet_length(payload::text)) from gah_private.dashboard_commands where channel_id = (p_body->>'channel_id')::uuid and not completed), 0) + octet_length(coalesce((p_body->'payload')::text, '{}')) > 4194304 then
        raise exception 'dashboard_command_limit' using errcode = '22023'; end if;
    insert into gah_private.dashboard_commands(id, channel_id, action, payload, payload_hash)
        values ((p_body->>'request_id')::uuid, (p_body->>'channel_id')::uuid, p_action, coalesce(p_body->'payload', '{}'), encode(sha256(convert_to(coalesce(p_body->'payload', '{}')::text, 'UTF8')), 'hex'));
    return to_jsonb(true);
end;
$$;
revoke all on function public.cloud_dashboard(text, jsonb, text) from public;
grant execute on function public.cloud_dashboard(text, jsonb, text) to anon, authenticated;
commit;
