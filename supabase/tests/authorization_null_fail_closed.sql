-- An active member with no private-node grant must not mutate another member's run.
\o /dev/null
begin;
\set ws '92000000-0000-0000-0000-000000000001'
\set owner '91000000-0000-0000-0000-000000000001'
\set holder '91000000-0000-0000-0000-000000000002'
\set stranger '91000000-0000-0000-0000-000000000003'
\set repo '93000000-0000-0000-0000-000000000001'
\set task '94000000-0000-0000-0000-000000000001'
create function pg_temp.ok(value boolean, message text) returns void
language plpgsql as $$ begin if value is distinct from true then raise exception '%', message; end if; end $$;
create function pg_temp.denied(query text, expected_message text) returns boolean language plpgsql as $$
begin
    execute query;
    return false;
exception when insufficient_privilege then return sqlerrm = expected_message;
end $$;
grant execute on all functions in schema pg_temp to authenticated;
insert into auth.users (id) values (:'owner'), (:'holder'), (:'stranger');
insert into public.workspaces (id, name) values (:'ws', 'NULL authorization regression');
insert into gah_private.memberships (workspace_id, user_id, role) values
    (:'ws', :'owner', 'owner'), (:'ws', :'holder', 'member'), (:'ws', :'stranger', 'member');
insert into gah_private.entitlements (user_id, valid_until) values
    (:'owner', now() + interval '1 day'), (:'holder', now() + interval '1 day'), (:'stranger', now() + interval '1 day');
insert into public.nodes (workspace_id, node_id, owner_id, display_name, advertised_url, version, schema_digest, transport_mode)
values (:'ws', 'private', :'holder', 'Private node', 'https://node.example.com', '1', 'test', 'authenticated_remote');
insert into public.repositories (workspace_id, repo_id, source, path) values (:'ws', :'repo', 'github.com', 'example/repo');
insert into public.tasks (workspace_id, task_id, repo_id, kind, ref) values (:'ws', :'task', :'repo', 'issue', '1');
insert into public.work_items (workspace_id, work_id, node_id, profile, state, requester_id, task_id, task_generation, leased_until)
values (:'ws', 'running', 'private', 'web', 'running', :'holder', :'task', 1, now() + interval '1 hour'),
    (:'ws', 'orphan', 'private', 'web', 'failed', :'holder', :'task', 1, null);
insert into public.file_locks (workspace_id, repo_id, scope, path_key, path, work_id, task_id, owner_id, generation)
values (:'ws', :'repo', 'main', 'orphan.rs', 'orphan.rs', 'orphan', :'task', :'holder', 1);
set local role authenticated;
select set_config('request.jwt.claims', json_build_object('sub', :'stranger', 'role', 'authenticated')::text, true) \gset
select pg_temp.ok(auth.uid() = :'stranger', 'JWT claims must identify the ordinary member');
select pg_temp.ok(gah_private.can_access(:'ws'), 'the member must have active workspace access');
select pg_temp.ok(gah_private.node_permission(:'ws', 'private') is null, 'the member must have NULL node permission');
select pg_temp.ok(pg_temp.denied(format('select public.force_cloud_takeover(%L, %L)', :'ws', :'task'), 'takeover_denied'),
    'NULL node permission must reject forced takeover with 42501/takeover_denied');
select pg_temp.ok(pg_temp.denied(format('select public.resolve_cloud_file_locks(%L, %L, %L)', :'ws', 'orphan', 'preserved'), 'resolve_denied'),
    'NULL node permission must reject orphan-lock resolution with 42501/resolve_denied');
reset role;
select pg_temp.ok((select stop_requested_at is null from public.work_items where workspace_id = :'ws' and work_id = 'running'),
    'denied takeover must not stop the running job');
select pg_temp.ok((select handoff_kind is null and handoff_to is null from public.tasks where workspace_id = :'ws' and task_id = :'task'),
    'denied takeover must not reserve the task');
select pg_temp.ok((select resolved_at is null and resolution is null from public.file_locks where workspace_id = :'ws' and work_id = 'orphan'),
    'denied resolution must leave the orphan lock intact');
select pg_temp.ok(not exists (select 1 from public.events where workspace_id = :'ws'), 'denied operations must not emit mutation events');
-- A workspace owner still has authority even without a grant on this private node.
set local role authenticated;
select set_config('request.jwt.claims', json_build_object('sub', :'owner', 'role', 'authenticated')::text, true) \gset
select pg_temp.ok(gah_private.node_permission(:'ws', 'private') is null, 'owner control must also exercise NULL node permission');
select public.force_cloud_takeover(:'ws', :'task', :'stranger');
select pg_temp.ok(public.resolve_cloud_file_locks(:'ws', 'orphan', 'preserved') = 1, 'workspace owner must still resolve locks');
reset role;
select pg_temp.ok((select stop_requested_at is not null from public.work_items where workspace_id = :'ws' and work_id = 'running'),
    'authorized takeover must still stop the running job');
select pg_temp.ok((select handoff_kind = 'approved' and handoff_to = :'stranger' from public.tasks where workspace_id = :'ws' and task_id = :'task'),
    'authorized takeover must still reserve the task');
rollback;
\o
\echo 'NULL node permission authorization checks passed'
