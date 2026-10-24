# Closed beta provisioning

Only the project owner provisions Auth accounts. Workspace owners invite those accounts through the app. An Auth account alone grants no workspace access.

## Disable public sign-ups

1. In the Supabase dashboard, select the intended project.
2. Open **Authentication → Providers → Email**.
3. Uncheck **Allow new users to sign up**, then save.
4. Reopen the page and verify that the setting remains unchecked.
5. From an owner terminal, submit this request with the project's URL and anon key supplied privately:

```sh
curl --silent --show-error --include "$GAH_SUPABASE_URL/auth/v1/signup" \
  --header "apikey: $GAH_SUPABASE_ANON_KEY" \
  --header 'Content-Type: application/json' \
  --data '{"email":"closed-beta-probe@example.test","password":"BetaProbe-Only-123!"}'
```

Verify HTTP 4xx and `signup_disabled` in the response. In **Authentication → Users**, verify that the probe email has no account.

The committed `supabase/config.toml` disables sign-ups under `[auth]`, `[auth.email]`, and `[auth.sms]` for local development.
Local configuration does not change hosted settings. The owner must complete the dashboard steps separately.

## Add the owner and testers to Auth

In **Authentication → Users → Add user → Create new user**, enter the email and a unique password.
Enable **Auto Confirm User**, then create the user. Record the user UUID privately and deliver credentials through a private channel.
Repeat for each tester before sending a workspace invitation.

Alternatively, use [`auth.admin.createUser`](https://supabase.com/docs/reference/javascript/auth-admin-createuser) from a trusted server or owner terminal:

```js
import { createClient } from '@supabase/supabase-js';

const admin = createClient(process.env.GAH_SUPABASE_URL, process.env.GAH_SUPABASE_SERVICE_ROLE_KEY, {
  auth: { persistSession: false, autoRefreshToken: false },
});
const { data, error } = await admin.auth.admin.createUser({
  email: process.env.TESTER_EMAIL,
  password: process.env.TESTER_INITIAL_PASSWORD,
  email_confirm: true,
});
if (error) throw error;
console.log(data.user.id);
```

Supply credentials through private environment variables. Keep service-role keys out of browsers, workers, repository files, and issue comments.
Have each tester change their initial password after signing in.

## Bootstrap the workspace owner

The exact signature is `gah_private.bootstrap_workspace_owner(p_workspace uuid, p_user uuid)`.
It promotes an existing member. It does not create the workspace, membership, or entitlement.
The migration revokes execution from `anon` and `authenticated` and grants it to `service_role`.

In the dashboard SQL editor as `postgres`, replace the placeholders with the intended workspace and provisioned owner UUIDs:

```sql
begin;
-- For a new workspace, choose its UUID and create it first.
insert into public.workspaces (id, name)
values ('<workspace_uuid>'::uuid, 'Beta workspace');

insert into gah_private.memberships (workspace_id, user_id, display_name)
values ('<workspace_uuid>'::uuid, '<owner_user_uuid>'::uuid, 'Owner');

select gah_private.bootstrap_workspace_owner(
  '<workspace_uuid>'::uuid,
  '<owner_user_uuid>'::uuid
);
commit;
```

For an existing workspace or member, omit its insert. Use only the bootstrap function to assign the owner role.
It raises `workspace_member_required` for a missing member and `workspace_already_has_owner` for a different existing owner.

Hosted access also requires a current entitlement for the owner and each tester. In the SQL editor, grant the approved beta period:

```sql
insert into gah_private.entitlements (user_id, valid_until)
values ('<provisioned_user_uuid>'::uuid, now() + interval '30 days')
on conflict (user_id) do update
set valid_until = excluded.valid_until, revoked_at = null;
```

Use the approved expiration instead of `30 days` when the beta period differs. Keep hosted entitlement enforcement enabled.

## Invite and accept in the app

1. In the enterprise dashboard, sign in as the workspace owner under **Settings → Cloud hosting**. Select the workspace.
2. Enter the provisioned tester email under **Teammate account**, then select **Invite to workspace**.
3. Sign in as that tester. In the **Workspace invitation** dialog, select **Join workspace**.
4. Verify that the tester appears as a member and can access the workspace.

The app uses `manage_workspace_members` for invite and accept, and `pending_workspace_invites` to find the tester's invitation.
The equivalent RPC calls use the signed-in user's client:

```js
// Owner session.
const { error: inviteError } = await supabase.rpc('manage_workspace_members', {
  p_workspace: workspaceId, p_action: 'invite', p_email: testerEmail,
});
if (inviteError) throw inviteError;
// Tester session. Use the selected pending invitation's workspace_id and id.
const { data: invitations, error } = await supabase.rpc('pending_workspace_invites');
if (error) throw error;
const invitation = invitations.find(item => item.workspace_id === workspaceId);
if (!invitation) throw new Error('No pending workspace invitation');
const { error: acceptError } = await supabase.rpc('manage_workspace_members', {
  p_workspace: invitation.workspace_id, p_action: 'accept', p_code: invitation.id,
});
if (acceptError) throw acceptError;
```

Verify each RPC error before proceeding. Invitations expire after 24 hours and bind to the recipient's Auth user UUID.
An unknown email raises `22023/account_not_found` on invite.
An invalid code or different recipient raises `22023/invite_invalid_or_expired` on accept.
Without a signed-in subject, accept raises `42501/sign_in_required`.
An accepted invitation grants membership. The tester also needs a current entitlement for hosted access.

## Local regression and owner sign-off

Install Docker Desktop, Supabase CLI, `psql`, and Node.js 18 or newer. Then run:

```sh
bash scripts/test-supabase-migrations.sh
node --test scripts/supabase-hygiene.test.mjs
```

The runner starts a disposable local stack with separate ports and copies of the committed migrations and configuration.
It resets that stack and submits an unknown-email signup through GoTrue with the local anon key.
The response must have HTTP 4xx, `signup_disabled`, and the message `Signups not allowed for this instance`.
SQL assertions verify that the email has no Auth row, invitations reject unknown accounts, and unknown subjects cannot redeem a valid code.
A provisioned tester must still accept that code. The runner rolls back SQL fixtures and removes its local stack on exit.

Before closing [issue #1603](https://github.com/Kh1ng/git-agent-harness/issues/1603), the owner must record this line in an issue comment:

> Hosted setting verified by owner: &lt;name&gt;, &lt;UTC date/time&gt;. Authentication → Providers → Email → Allow new users to sign up is unchecked after saving. Unknown-email sign-up rejected and no Auth user created. Provisioned tester invitation accepted. Evidence: &lt;redacted result&gt;.

Hosted verification status: **pending owner sign-off**. Local results do not verify the hosted setting.
Keep project identifiers, URLs, user UUIDs, emails, and keys out of public evidence.
