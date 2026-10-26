# Stateful scenario forge

`ScenarioHarness::stateful_forge()` installs fake `gh` and `glab` executables in the harness PATH.
Both use one JSON state file throughout the scenario.
Python 3 and its standard library are required. No Python packages or Rust dependencies are added.
Python handles JSON and file locking without shell string parsing or a second compiled test executable.

```rust
let mut harness = ScenarioHarness::new("github");
let forge = harness.stateful_forge();
forge.pull(7, "gah/change", "source-sha", "Draft: TICKET-7 change");
forge.update(|state| {
    state["checks"]["source-sha"] = serde_json::json!([
        {"status": "completed", "conclusion": "success"}
    ]);
});
let first = harness.run_one_loop().unwrap();
let state = harness.stateful_forge().state();
```

`state_path` is an inspectable JSON file. `update` atomically replaces it between iterations, while no command runs.
`issues` and `pulls` contain normalized records keyed by number. Their `comments`, `reviews`, labels, and assignees persist after writes.
GitHub shares issue and PR numbers. GitLab uses separate issue and MR counters.
`branches` maps branch names to SHAs. PR creation resolves an unseeded branch from the local Git checkout.
`checks` and `pipelines` map head SHAs to provider records. A changed head has no checks unless the test supplies them.
`versions`, `review_comments`, and `discussions` accept provider-shaped fixtures for positioned review scenarios.

`calls` records each executable name and exact argv as a JSON array. `commands.log` supplies the readable companion log.
Unknown commands, flags, endpoints, and jq expressions fail with nonzero status.

| Operations | GitHub commands | GitLab commands |
|---|---|---|
| Identity | `gh api user --jq .login` | `glab api user` |
| Issues | `gh api repos/{repo}/issues[/N]`, `gh issue close` | `glab api projects/{id}/issues[/N]` |
| PR creation and discovery | `gh pr create/view/list`, `gh api repos/{repo}/pulls`, `gh api search/issues` | `glab api projects/{id}/merge_requests[/N]`, `glab mr list` |
| Comments and claims | Issue `/comments`, `/issues/comments/ID`, `/assignees` | Issue or MR `/notes[/ID]`, issue or MR updates |
| Labels | Issue `/labels[/name]` | Issue or MR updates with `labels`, `add_labels`, `remove_labels` |
| Reviews | `gh pr review`, PR `/reviews` and `/comments`, issue `/timeline` | MR `/versions[/ID]`, `/discussions[/ID[/notes]]` |
| CI | Commit `/check-runs`, PR `statusCheckRollup` | MR `/pipelines`, `head_pipeline` |
| Merge | `gh pr ready`, `gh pr merge --squash --match-head-commit` | MR update, `/merge` with squash, SHA, optional auto-merge |
| Relationships | Issue `/sub_issues` | Issue `/links` |
| Branches | `/git/refs`, `/git/ref/heads/{branch}` | `/repository/branches[/name]` |

The parser accepts field flags, explicit methods, hostname/repo flags, JSON stdin, JSON selection, and the jq expressions used by production.
Lists honor `page` and `per_page`. GitHub `--paginate` emits successive JSON pages or filtered claim records.
Merge rejects a stale SHA or draft, records a deterministic squash commit, updates the target branch, and optionally removes the source branch.
GitLab auto-merge waits for a successful current-head pipeline. The next MR read applies the merge.
The forge does not reproduce provider permissions, remote Git objects, network behavior, or full jq.

The state clock starts at construction time and stays fixed unless the test changes `now`.
Commands also honor `GAH_TEST_NOW`, the subprocess clock from #1515.

For a deterministic competing actor, `effects` applies JSON-path replacements before the specified matching API call.
Optional `argv_contains` entries restrict the match to commands that contain those arguments.

```json
{
  "endpoint": "repos/owner/repo/issues/7",
  "method": "GET",
  "at": 3,
  "patches": [{"path": ["pulls", "7", "assignees"], "value": ["gah-bot", "a-person"]}]
}
```

The self-tests in `tests.rs` and `commands.rs` run through `fake_backend_harness` and use production argv and JSON shapes.
They replay both CLI list forms recorded by `FakeBackend` and reject unsupported command paths.
The migrated claim tests exercise the real controller, including lease takeover and publication suppression.
