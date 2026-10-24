# Self-hosted Supabase setup

Parasaur can coordinate machines through a Supabase project. In self-hosted
mode, the project is your own. This guide builds that project from the files
in this repository. You do not need the private enterprise repository; see
[Optional enterprise integration](ENTERPRISE.md).

The `orchestration_mode` setting has three values:

- `local` — no cloud project; the default.
- `hosted` — the shared Parasaur project; paid, closed beta.
- `self_hosted` — your own Supabase project; this guide.

## What you need

- Parasaur on each machine. See [Getting started](GETTING_STARTED.md).
- The Supabase CLI. See the [Supabase CLI guide](https://supabase.com/docs/guides/cli/getting-started).
- A project on the Supabase platform (the free tier works), or Docker to
  run the same project on your machine.

## 1. Create the project

### On the Supabase platform

1. Create a project at <https://supabase.com/dashboard>.
2. Save the project ref. The dashboard shows it under Project Settings.
3. Link the CLI to your project. Run this in a clone of this repository:

```bash
supabase link --project-ref <your-project-ref>
```

The link state lives in `supabase/.temp/`. Git ignores that directory. Only
your CLI can push to your project.

### Locally with Docker

Run this from the repository root:

```bash
supabase start
```

The command starts the containers described in `supabase/config.toml`. That
file is part of this repository. Public sign-ups are disabled in it.

## 2. Apply the migrations

The schema is every file under `supabase/migrations/`.

### On the Supabase platform

Preview first. Then push:

```bash
supabase db push --dry-run
supabase db push
```

The push applies every migration to your linked project. It never touches
the hosted Parasaur project.

### Locally

Recreate the local database and apply every migration:

```bash
supabase db reset --local
```

## 3. Create the first user

Public sign-ups are disabled. No one can create an account alone. You create
each account yourself.

- Platform: open **Authentication → Users → Add user**.
- Local: open Studio at <http://127.0.0.1:54323>, then the same page.

Enter an email and a password. Confirm the email during creation. Save.
Copy the user ID. It is a UUID.

Keep the sign-up switch off. Local: `supabase/config.toml` sets
`enable_signup = false`. Platform: keep the new-user sign-up toggle off in
the Authentication settings.

## 4. Bootstrap the workspace owner

A workspace is one shared set of tables. Sign-ups do not create workspaces
or members. Bootstrap the first owner yourself. Do these steps once.

Open the **SQL Editor**: on the platform, or in local Studio at
<http://127.0.0.1:54323>.

Create the workspace. Copy the returned ID:

```sql
insert into public.workspaces (name) values ('Home') returning id;
```

Add the user as a member:

```sql
insert into gah_private.memberships (workspace_id, user_id, display_name)
values ('<workspace-id>', '<user-id>', 'Owner');
```

Promote the member to owner:

```sql
select gah_private.bootstrap_workspace_owner(
  '<workspace-id>'::uuid,
  '<user-id>'::uuid
);
```

Turn off the entitlement requirement:

```sql
update gah_private.deployment set requires_entitlement = false;
```

Why: the schema starts with `requires_entitlement = true`, the hosted paid
model. Your project sells nothing, so no entitlement can exist. Skip this
step and every workspace access check fails.

The bootstrap function reports plain errors:

- `workspace_not_found`: the workspace ID is wrong.
- `workspace_member_required`: add the membership row first.
- `workspace_already_has_owner`: another owner already exists.

## 5. Get the URL and the anon key

### On the Supabase platform

Open **Project Settings → API**. Copy the **Project URL** and the **anon
public** key.

### Locally

```bash
supabase status -o env
```

The URL is the `API_URL` value. The anon key is the `ANON_KEY` value.

Keep the service-role key out of Parasaur. The CLI never reads it. Anyone who
holds it owns the whole database.

## 6. Point Parasaur at the project

Open `~/.config/gah/config.toml`. Set the mode under `[defaults]`:

```toml
[defaults]
orchestration_mode = "self_hosted"
```

Export the credentials in each interactive shell that runs Parasaur:

```bash
export GAH_SUPABASE_URL="<your-project-url>"
export GAH_SUPABASE_ANON_KEY="<your-anon-key>"
```

A blank value counts as unset.

Services do not read your shell. Each one loads a fixed env file, and the
`export` lines above are for your shell only. In a service env file, the two
lines must be plain assignments:

```bash
GAH_SUPABASE_URL="<your-project-url>"
GAH_SUPABASE_ANON_KEY="<your-anon-key>"
```

Add them to the file your service reads:

- `~/.config/gah/gah-loop.env` — the controller loop. On Linux,
  `gah-loop@.service` loads it as a systemd `EnvironmentFile`; the macOS
  worker LaunchAgent sources it in bash. Run `chmod 600` on it.
- `/etc/gah/server.env` — the central server on Linux.
  `gah-server.service` loads it as a systemd `EnvironmentFile`. The
  installer creates it.
- `~/.config/gah/server.env` — the central server on macOS. The server
  LaunchAgent sources this file, not `gah-loop.env`. Without the
  credentials here, the server's CLI calls fail cloud-mode validation. Run
  `chmod 600` on it.

## 7. Start a central node and a worker

Build the CLI on each machine, and repeat step 6 on each machine:

```bash
cargo build --release --bin gah
```

On the central machine:

```bash
target/release/gah setup --role central
```

On the worker machine:

```bash
target/release/gah setup --role worker --central-url <central-url>
```

The central dashboard prints the exact worker command under
**Settings → Add a Node**; paste it on the worker.

## 8. Verify

Run:

```bash
gah config show
```

Expected output:

```
orchestration_mode: self_hosted
GAH_SUPABASE_URL: [REDACTED]
GAH_SUPABASE_ANON_KEY: [REDACTED]
current_manager: (unset)
```

Values never print. A set credential prints as `[REDACTED]`. A missing one
prints as `(unset)`. `current_manager` prints your manager or `(unset)`.

For scripts:

```bash
gah config show --json --full
```

The JSON keeps the same redaction.

## 9. Troubleshooting

**`no config found; set GAH_CONFIG or create ...`**
No config file exists. Create `~/.config/gah/config.toml`. Or point
`GAH_CONFIG` at a file.

**`parsing ...`**
The TOML is invalid. Check the quotes and brackets. The error names the file.

**`orchestration_mode = "self_hosted" requires GAH_SUPABASE_URL; export
GAH_SUPABASE_URL before starting gah, or use [defaults] orchestration_mode =
"local"`**
Export the missing variable. The same error names `GAH_SUPABASE_ANON_KEY`
when the key is missing. A blank value counts as missing.

**Unknown mode value.**
The value must be `local`, `hosted`, or `self_hosted`.

**`gah config show` prints `(unset)`.**
The variable is missing in that shell. Export it. Or add it to the env file of
the service that runs Parasaur.

**A bootstrap error appears.**
See the error list in step 4.

**`supabase db push` fails.**
Check that you linked your own project ref. Run `supabase db push --dry-run`
and read the listed migrations before the real push.

## Not yet in the public repo

Two Edge Functions exist only in the private enterprise repository:
`workspace-members` and `node-health`. They are not in this repository. See
[Optional enterprise integration](ENTERPRISE.md).

The public repository contains the schema migrations, the config, and the
mode validation. The pieces that sync nodes through Supabase have not landed
here yet. Until they do, `self_hosted` mode validates your credentials at
startup and reports them redacted. Machines keep coordinating through the
local registry.
