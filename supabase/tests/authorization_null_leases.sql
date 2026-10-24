-- A stored running row without a lease must never grant ownership or release inference output.
\o /dev/null
begin;
\set ws '92000000-0000-0000-0000-000000000002'
\set owner '91000000-0000-0000-0000-000000000004'
\set device '96000000-0000-0000-0000-000000000001'
\set claim '97000000-0000-0000-0000-000000000001'
\set request '98000000-0000-0000-0000-000000000001'
create function pg_temp.ok(value boolean, message text) returns void
language plpgsql as $$ begin if value is distinct from true then raise exception '%', message; end if; end $$;
create function pg_temp.denied(query text, expected_message text) returns boolean language plpgsql as $$
begin
    execute query;
    return false;
exception when insufficient_privilege then return sqlerrm = expected_message;
end $$;
grant execute on all functions in schema pg_temp to anon;
insert into auth.users (id) values (:'owner');
insert into public.workspaces (id, name) values (:'ws', 'NULL lease regression');
insert into gah_private.memberships (workspace_id, user_id, role) values (:'ws', :'owner', 'owner');
insert into gah_private.entitlements (user_id, valid_until) values (:'owner', now() + interval '1 day');
insert into public.nodes (workspace_id, node_id, owner_id, display_name, advertised_url, version, schema_digest, transport_mode, profiles)
values (:'ws', 'lease-test', :'owner', 'Lease test', 'https://node.example.com', '1', 'test', 'authenticated_remote', array['web']);
-- A fixed synthetic local credential; only its hash is persisted, and output is suppressed.
insert into gah_private.devices (id, workspace_id, node_id, token_hash)
values (:'device', :'ws', 'lease-test', encode(sha256(convert_to(repeat('a', 72), 'UTF8')), 'hex'));
insert into public.work_items (workspace_id, work_id, node_id, profile, state, requester_id, device_id, claim_id, memory_mb, leased_until)
values (:'ws', 'null-lease', 'lease-test', 'web', 'running', :'owner', :'device', :'claim', 64, null);
insert into gah_private.inference_requests (id, workspace_id, work_id, inference_node_id, compute_device_id, request_key, path, body, response, status, state)
values (:'request', :'ws', 'null-lease', 'lease-test', :'device', gen_random_uuid(), 'responses', '{}', 'test output', 200, 'completed');
select pg_temp.ok(gah_private.job_allowed(w), 'fixture must pass all resource authorization checks')
from public.work_items w where workspace_id = :'ws' and work_id = 'null-lease';
set local role anon;
select (public.check_cloud_ownership(repeat('a', 72), 'null-lease', :'claim')->>'owned')::boolean is false as ownership_denied \gset
select pg_temp.denied(format('select public.poll_cloud_inference(repeat(''a'', 72), %L)', :'request'), 'inference_access_denied') as inference_denied \gset
reset role;
-- Evaluate both before asserting, so the pre-fix run reports both failures.
\o
\echo 'NULL lease denials: ownership=' :ownership_denied 'inference=' :inference_denied
\o /dev/null
select pg_temp.ok(:'ownership_denied'::boolean, 'NULL lease must deny cloud ownership');
select pg_temp.ok(:'inference_denied'::boolean, 'NULL lease must reject inference polling with 42501/inference_access_denied');
select pg_temp.ok((select response = 'test output' from gah_private.inference_requests where id = :'request'),
    'denied inference polling must preserve the pending response');
update public.work_items set leased_until = now() + interval '1 hour' where workspace_id = :'ws' and work_id = 'null-lease';
set local role anon;
select pg_temp.ok((public.check_cloud_ownership(repeat('a', 72), 'null-lease', :'claim')->>'owned')::boolean,
    'a live lease must still grant ownership');
select pg_temp.ok(public.poll_cloud_inference(repeat('a', 72), :'request')->>'response' = 'test output',
    'a live lease must still release inference output');
reset role;
rollback;
\o
\echo 'NULL lease ownership and inference authorization checks passed'
