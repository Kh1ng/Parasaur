-- Signup must not create an account; invitations bind to provisioned Auth users.
\o /dev/null
begin;
\set ws '96000000-0000-0000-0000-000000000001'
\set owner '97000000-0000-0000-0000-000000000001'
\set tester '97000000-0000-0000-0000-000000000002'
\set unknown '97000000-0000-0000-0000-000000000003'
\set code '98000000-0000-0000-0000-000000000001'
create function pg_temp.ok(value boolean, message text) returns void
language plpgsql as $$ begin if value is distinct from true then raise exception '%', message; end if; end $$;
create function pg_temp.denied(query text, expected_state text, expected_message text) returns boolean
language plpgsql as $$
begin
    execute query;
    return false;
exception when others then
    return sqlstate = expected_state and sqlerrm = expected_message;
end $$;
grant execute on all functions in schema pg_temp to authenticated;
select pg_temp.ok(not exists (select 1 from auth.users where lower(email) = 'closed-beta-unknown@example.test' or id = :'unknown'),
    'unknown email must have no auth.users row after rejected signup');
insert into auth.users (id, email) values (:'owner', 'closed-beta-owner@example.test'), (:'tester', 'closed-beta-tester@example.test');
insert into public.workspaces (id, name) values (:'ws', 'Closed beta regression');
insert into gah_private.memberships (workspace_id, user_id) values (:'ws', :'owner');
select gah_private.bootstrap_workspace_owner(:'ws', :'owner');
insert into gah_private.entitlements (user_id, valid_until) values (:'owner', now() + interval '1 day'), (:'tester', now() + interval '1 day');

set local role authenticated;
select set_config('request.jwt.claims', json_build_object('sub', :'owner', 'role', 'authenticated')::text, true) \gset
select pg_temp.ok(pg_temp.denied(format('select public.manage_workspace_members(%L, %L, %L)',
    :'ws', 'invite', 'closed-beta-unknown@example.test'), '22023', 'account_not_found'),
    'inviting an unknown email must raise 22023/account_not_found');
select pg_temp.ok(public.manage_workspace_members(:'ws', 'invite', 'closed-beta-tester@example.test')->>'invited' = 'true',
    'owner must be able to invite a provisioned tester');
reset role;
-- A missing Auth user cannot have an invitation row because user_id is a foreign key.
-- Use a real pending code to prove that even a synthetic unknown subject cannot redeem it.
update gah_private.workspace_invites set code = :'code' where workspace_id = :'ws' and user_id = :'tester';
set local role authenticated;
select set_config('request.jwt.claims', json_build_object('sub', :'unknown', 'role', 'authenticated')::text, true) \gset
select pg_temp.ok(auth.uid() = :'unknown', 'acceptance check must use the unknown subject');
select pg_temp.ok(pg_temp.denied(format('select public.manage_workspace_members(%L, %L, p_code => %L)',
    :'ws', 'accept', :'code'), '22023', 'invite_invalid_or_expired'),
    'unknown subject must not accept a valid invitation: 22023/invite_invalid_or_expired');
select set_config('request.jwt.claims', '{"role":"authenticated"}', true) \gset
select pg_temp.ok(pg_temp.denied(format('select public.manage_workspace_members(%L, %L, p_code => %L)',
    :'ws', 'accept', :'code'), '42501', 'sign_in_required'),
    'acceptance without a signed-in subject must raise 42501/sign_in_required');
reset role;
select pg_temp.ok(not exists (select 1 from gah_private.memberships where workspace_id = :'ws' and user_id <> :'owner'),
    'rejected invitations and acceptance must not add memberships');
select pg_temp.ok((select count(*) = 1 from gah_private.workspace_invites where workspace_id = :'ws' and code = :'code' and user_id = :'tester'),
    'rejected acceptance must leave the provisioned tester invitation intact');
set local role authenticated;
select set_config('request.jwt.claims', json_build_object('sub', :'tester', 'role', 'authenticated')::text, true) \gset
select pg_temp.ok(public.manage_workspace_members(:'ws', 'accept', p_code => :'code')->>'active' = 'true',
    'provisioned tester must still be able to accept their invitation');
reset role;
select pg_temp.ok(exists (select 1 from gah_private.memberships where workspace_id = :'ws' and user_id = :'tester' and role = 'member'),
    'accepted tester must become a workspace member');
rollback;
\o
\echo 'Closed beta SQL passed: unknown email absent; invite 22023/account_not_found; accept 22023/invite_invalid_or_expired; unsigned 42501/sign_in_required; provisioned tester accepted'
