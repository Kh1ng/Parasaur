#!/usr/bin/env bash
# Run: bash scripts/test-supabase-migrations.sh (Supabase CLI, Docker, psql, Node.js).
# Resets only a disposable local stack in a temp copy; never links or pushes a project.
set -euo pipefail
for command in supabase docker psql node; do
  command -v "$command" >/dev/null || { echo "Required command missing: $command" >&2; exit 1; }
done
docker info >/dev/null
root="$(cd "$(dirname "$0")/.." && pwd)"
stack="$(mktemp -d "${TMPDIR:-/tmp}/gah-supabase-test.XXXXXX")"
cleanup() {
  (cd "$stack" && supabase stop --no-backup) >/dev/null 2>&1 || true
  rm -rf "$stack"
}
trap cleanup EXIT
mkdir -p "$stack/supabase"
cp -R "$root/supabase/migrations" "$root/supabase/tests" "$stack/supabase/"
# Separate container names and ports from any developer's existing local stack.
port=$((40000 + $$ % 20000))
sed -E "s/^project_id = .*/project_id = \"gah-auth-test-$$\"/; s/54322/$port/; s/54320/$((port + 1))/; s/54321/$((port + 2))/; s/54323/$((port + 3))/" \
  "$root/supabase/config.toml" > "$stack/supabase/config.toml"
cd "$stack"
supabase db start
supabase db reset --local
# Read only the local DB URL; do not print the other status values or evaluate shell output.
db_url="$(supabase status -o json | node -e 'let s=""; process.stdin.on("data", d => s += d); process.stdin.on("end", () => { const url = JSON.parse(s).DB_URL; if (!url) process.exit(1); process.stdout.write(url); });')"
failed=0
for test in supabase/tests/authorization_null*.sql; do
  psql -X -q -v ON_ERROR_STOP=1 "$db_url" -f "$test" || failed=1
done
if ((failed)); then exit 1; fi
printf 'Supabase local reset and authorization regression passed (%s migrations)\n' "$(find supabase/migrations -name '*.sql' | wc -l | tr -d ' ')"
