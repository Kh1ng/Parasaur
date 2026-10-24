use crate::config::{self, GahConfig, Profile};
use crate::sync::{self, SyncMr};
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

pub fn run(
    profile_name: Option<&str>,
    config_path: Option<&str>,
    older_than_days: Option<u64>,
    dry_run: bool,
) -> Result<()> {
    let cfg = config::load(config_path)?;
    let profiles = selected_profiles(&cfg, profile_name)?;

    for (name, profile) in profiles {
        prune_profile(&cfg, &name, profile, older_than_days, dry_run, true, None)?;
    }
    Ok(())
}

/// Run the same conservative maintenance used by `gah prune` for one
/// controller profile. The controller supplies its bounded active-MR and
/// live-claim observations so this never needs full provider history.
pub fn run_automatic(
    cfg: &GahConfig,
    profile_name: &str,
    ledger_entries: &[crate::ledger::LedgerEntry],
    snapshot: &crate::status::StatusSnapshot,
) -> Result<()> {
    let profile = config::get_profile(cfg, profile_name)?;
    let active_mrs =
        (snapshot.observations.sync.status == "ok").then_some(snapshot.merge_requests.as_slice());
    let active_claims = (!snapshot
        .errors
        .iter()
        .any(|error| error.subsystem == "claims" && error.incomplete_snapshot))
    .then_some(snapshot.active_claims.as_slice());
    let observed = automatic_worktree_sets(
        ledger_entries,
        profile_name,
        &profile.repo_id,
        active_mrs,
        active_claims,
    );
    prune_profile(
        cfg,
        profile_name,
        profile,
        None,
        false,
        false,
        Some(&observed),
    )
}

#[derive(Default)]
struct AutomaticWorktreeSets {
    done: HashSet<String>,
    abandoned_open: HashSet<String>,
    protected: HashSet<String>,
}

fn automatic_worktree_sets(
    ledger_entries: &[crate::ledger::LedgerEntry],
    profile_name: &str,
    repo_id: &str,
    active_mrs: Option<&[crate::sync::SyncMrJson]>,
    active_claims: Option<&[crate::status::ActiveClaimSnapshot]>,
) -> AutomaticWorktreeSets {
    let dispatch_prefixes = [format!("gah/{repo_id}-"), format!("gah/exp/{repo_id}-")];
    let published = ledger_entries
        .iter()
        .filter(|entry| entry.profile == profile_name && entry.repo_id == repo_id)
        .filter(|entry| entry.mr_created)
        .filter_map(|entry| Some((entry.branch.as_deref()?, entry.work_id.as_deref()?)))
        .filter(|(branch, _)| {
            dispatch_prefixes
                .iter()
                .any(|prefix| branch.starts_with(prefix))
        })
        .collect::<Vec<_>>();
    let active_branches = active_mrs.map(|mrs| {
        mrs.iter()
            .map(|mr| mr.branch.as_str())
            .collect::<HashSet<_>>()
    });

    let mut sets = AutomaticWorktreeSets::default();
    match active_claims {
        Some(claims) => {
            let live_work_ids = claims
                .iter()
                .map(|claim| crate::work_claim::normalize_work_identity(&claim.work_id))
                .collect::<HashSet<_>>();
            sets.protected.extend(
                published
                    .iter()
                    .filter(|(_, work_id)| {
                        live_work_ids.contains(&crate::work_claim::normalize_work_identity(work_id))
                    })
                    .map(|(branch, _)| worktree_name_for_branch(branch)),
            );
        }
        None => {
            // Claim state is part of the same snapshot. If it was unreadable,
            // retain every published dispatch worktree rather than guessing
            // that none has a live owner.
            sets.protected.extend(
                published
                    .iter()
                    .map(|(branch, _)| worktree_name_for_branch(branch)),
            );
        }
    }

    let Some(active_branches) = active_branches else {
        // Absence is terminal evidence only when the active-MR observation
        // completed successfully.
        return sets;
    };
    sets.done.extend(
        published
            .iter()
            .filter(|(branch, _)| !active_branches.contains(branch))
            .map(|(branch, _)| worktree_name_for_branch(branch)),
    );
    sets.abandoned_open.extend(
        published
            .iter()
            .filter(|(branch, _)| active_branches.contains(branch))
            .map(|(branch, _)| worktree_name_for_branch(branch))
            .filter(|name| !sets.protected.contains(name)),
    );
    sets
}

fn prune_profile(
    cfg: &GahConfig,
    name: &str,
    profile: &Profile,
    older_than_days: Option<u64>,
    dry_run: bool,
    announce: bool,
    observed: Option<&AutomaticWorktreeSets>,
) -> Result<()> {
    // CLI --older-than overrides the per-profile retention window.
    let retention = older_than_days.unwrap_or_else(|| profile.effective_prune_older_than_days());
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(retention.saturating_mul(86_400)))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    if announce {
        println!("[{name}] retention={retention}d");
    }
    prune_sessions(profile, cutoff, dry_run)?;
    crate::build_cache::prune_inactive(&profile.artifact_root, dry_run)?;
    let provider_done;
    let empty = HashSet::new();
    let (done, abandoned_open, protected) = if let Some(observed) = observed {
        (
            &observed.done,
            &observed.abandoned_open,
            &observed.protected,
        )
    } else if announce {
        provider_done = done_worktree_names(profile);
        (&provider_done, &empty, &empty)
    } else {
        (&empty, &empty, &empty)
    };
    prune_worktrees(
        cfg,
        profile,
        cutoff,
        dry_run,
        done,
        abandoned_open,
        protected,
    )?;
    // Remote branch deletion still requires explicit full provider history;
    // recurring maintenance only reuses the controller observation above.
    if announce {
        prune_remote_gah_branches(profile, dry_run)?;
    }
    Ok(())
}

/// Deletes the remote branch for any MERGED/CLOSED_UNMERGED PR/MR whose
/// branch matches gah's own dispatch naming convention (`gah/...` --
/// `format!("gah/{}-{}", profile.repo_id, ts)` in dispatch/workflows).
/// Scoped to that prefix deliberately: this must never touch a
/// human-authored branch just because its PR happened to merge. GitHub/
/// GitLab's own "delete branch on merge" doesn't always fire (e.g. `gh pr
/// merge --admin` merges outside the normal UI flow), which is how ~280
/// stale branches accumulated on this repo before this existed.
fn prune_remote_gah_branches(profile: &Profile, dry_run: bool) -> Result<()> {
    let mrs = match sync::fetch_mrs(profile) {
        Ok(mrs) => mrs,
        Err(_) => return Ok(()), // best-effort, matches done_worktree_names
    };
    let push_url = profile.push_url()?;
    let pat = profile.pat();
    let repo_path = Path::new(&profile.local_path);
    for (branch, class) in remote_gah_branches_to_delete(&mrs) {
        if dry_run {
            println!("  would delete remote branch {branch} ({class})");
            continue;
        }
        match crate::worktree::delete_remote_branch(repo_path, branch, &push_url, &pat) {
            Ok(()) => println!("  deleted remote branch {branch} ({class})"),
            Err(err) => println!("  failed to delete remote branch {branch}: {err}"),
        }
    }
    Ok(())
}

/// Pure subset of `prune_remote_gah_branches`: which branches are safe to
/// delete on the remote, and why. Scoped to gah's own dispatch naming
/// convention (`gah/...`) deliberately -- this must never touch a
/// human-authored branch just because its PR happened to merge.
pub(crate) fn remote_gah_branches_to_delete(mrs: &[SyncMr]) -> Vec<(&str, &'static str)> {
    mrs.iter()
        .filter(|mr| mr.branch.starts_with("gah/"))
        .filter_map(|mr| match sync::classify(mr) {
            class @ ("MERGED" | "CLOSED_UNMERGED") => Some((mr.branch.as_str(), class)),
            _ => None,
        })
        .collect()
}

/// Resolve either one named profile or every configured profile, sorted.
/// Shared with anything that needs "operate on this profile, or all of
/// them" instead of a hardcoded host-local list (e.g. `gah watchdog-check`).
pub(crate) fn selected_profiles<'a>(
    cfg: &'a GahConfig,
    profile_name: Option<&str>,
) -> Result<Vec<(String, &'a Profile)>> {
    if let Some(name) = profile_name {
        return Ok(vec![(name.to_string(), config::get_profile(cfg, name)?)]);
    }
    let mut profiles: Vec<_> = cfg.profiles.iter().map(|(k, v)| (k.clone(), v)).collect();
    profiles.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(profiles)
}

/// Map a PR/MR branch name to the on-disk worktree directory name. Mirrors
/// `worktree::create` / `worktree::create_existing`, which call
/// `worktree_base.join(branch.replace('/', "-"))`. Pure and testable.
pub(crate) fn worktree_name_for_branch(branch: &str) -> String {
    branch.replace('/', "-")
}

/// Whether a worktree directory under the shared worktree base carries
/// this profile's repo id in the name. `worktree::create` writes the
/// dispatch branch with '/' replaced by '-', so the shapes are:
/// - `gah-{repo_id}-{seconds}-...`     dispatch (workflows/improve.rs)
/// - `gah-exp-{repo_id}-{seconds}-...` experiments (workflows/experiment.rs)
/// - `gah-chat-{repo_id}-{session}`    chat sessions (apps/server
///   chatSessions.ts: generated 8-hex/UUID, or any supplied
///   `[a-zA-Z0-9_-]{1,128}` id)
///
/// The `{repo_id}` segment is exact: repo ids may themselves contain '-'
/// (profiles `app` vs `app-web`), and the old plain
/// `starts_with("gah-{repo_id}-")` sweep applied this profile's retention
/// to another profile's worktrees, with no chance to see the other
/// profile's live-claim protection (#1547). For dispatch and experiment
/// names the generator's leading `{unix seconds}-` digit run is what makes
/// the segment exact: `gah-app-web-...` cannot parse as profile `app`'s
/// worktree because `web-` starts no timestamp.
///
/// Name matching is deliberately necessary but not sufficient. A name
/// like `gah-exp-app-{seconds}-...` is a valid experiment worktree of
/// profile `app` and a valid dispatch worktree of profile `exp-app`, and a
/// chat name matches every repo id the string could be split around --
/// so `prune_worktrees` additionally requires every candidate to resolve
/// to this profile's repository before removing it (see
/// `classify_worktree_ownership`).
// ponytail: two profiles like `app` and `exp-app` configured on the SAME
// repository remain ambiguous by name even after the ownership check --
// their worktrees share one common git dir. The upgrade path is
// recording the owning profile in the worktree metadata at creation.
pub(crate) fn worktree_name_matches_profile(name: &str, repo_id: &str) -> bool {
    profile_scoped_suffix(name, "gah-", repo_id).is_some_and(is_dispatch_timestamp)
        || profile_scoped_suffix(name, "gah-exp-", repo_id).is_some_and(is_dispatch_timestamp)
        || profile_scoped_suffix(name, "gah-chat-", repo_id).is_some_and(is_chat_session_id)
}

/// The suffix after `prefix{repo_id}-`, or `None` when the name does not
/// carry exactly this profile's repo_id as the segment following `prefix`.
fn profile_scoped_suffix<'a>(name: &'a str, prefix: &str, repo_id: &str) -> Option<&'a str> {
    name.strip_prefix(prefix)?
        .strip_prefix(repo_id)?
        .strip_prefix('-')
}

/// The dispatch/identity.rs `timestamp()` generator always emits
/// `"{unix seconds}-..."`, so the only invariant prune relies on is a
/// non-empty run of ASCII digits followed by '-'. The tail after that
/// separator is deliberately unconstrained: production worktrees exist
/// with 25, 29, 30 and 33 character hex tails (observed on central), so
/// requiring today's 32-char simple-UUID tail would silently disable
/// their cleanup. Cross-profile collisions a loose tail cannot rule out
/// are closed by the repository-ownership check, not by shape.
fn is_dispatch_timestamp(suffix: &str) -> bool {
    match suffix.split_once('-') {
        Some((seconds, _)) => {
            !seconds.is_empty() && seconds.bytes().all(|byte| byte.is_ascii_digit())
        }
        None => false,
    }
}

/// chatSessions.ts session ids: `newSessionId()` emits hex, the
/// coordinator issues `randomUUID()`, and callers may supply any
/// `[a-zA-Z0-9_-]{1,128}` id (e.g. `test-session`). The only total rule
/// is therefore "non-empty"; any shape constraint would make valid
/// current session ids permanently ineligible for prune. Cross-profile
/// safety for chat names comes from the repository-ownership check, not
/// from the session id.
fn is_chat_session_id(suffix: &str) -> bool {
    !suffix.is_empty()
}

/// What `git rev-parse --git-common-dir` says about a candidate worktree
/// directory's relationship to this profile's repository. Mandatory for
/// every candidate before removal (#1547): the naming rules above cannot
/// separate every cross-profile collision (`gah-exp-app-...` belongs to
/// both profiles `app` and `exp-app`; a chat name matches every repo id
/// the string could be split around), but two profiles on different
/// repositories can never share one common git dir.
enum WorktreeOwnership {
    /// A linked worktree of this profile's own repository: eligible for
    /// removal under the existing age/clean/terminal rules.
    Owned,
    /// A linked worktree of a different repository -- another profile's
    /// worktree sharing the worktree base. Retained and logged once;
    /// this profile's retention, live-claim view and the
    /// `remove_dir_all` fallback must never reach it.
    Foreign,
    /// Not a linked worktree at all: a plain non-git directory, a
    /// standalone `git init` left behind by an interrupted removal, or a
    /// worktree whose git admin metadata is already gone. Today's orphan
    /// handling applies unchanged.
    Orphan,
}

/// `git -C <dir> rev-parse --path-format=absolute <selector>`, or `None`
/// when git does not resolve it (not a repository, broken admin metadata).
/// Same invocation `worktree::lock_repo_git_state` uses, so linked-worktree
/// profile checkouts resolve to their main repository's `.git` here too.
fn rev_parse_absolute(dir: &Path, selector: &str) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", selector])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then(|| PathBuf::from(value))
}

/// Path equality that symlinks (macOS `/tmp`, `/var`) cannot fool. Falls
/// back to the raw comparison when either side cannot be canonicalized.
fn same_repository_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn classify_worktree_ownership(
    worktree: &Path,
    profile_common_dir: Option<&Path>,
) -> WorktreeOwnership {
    let Some(common_dir) = rev_parse_absolute(worktree, "--git-common-dir") else {
        return WorktreeOwnership::Orphan;
    };
    // A linked worktree keeps its admin state under the main repository's
    // `.git/worktrees/`, so its common dir lies outside itself. A directory
    // whose common dir is its own `.git` is a standalone repository -- the
    // orphaned state an interrupted removal leaves behind.
    if same_repository_path(&common_dir, &worktree.join(".git")) {
        return WorktreeOwnership::Orphan;
    }
    let Some(profile_common_dir) = profile_common_dir else {
        // This profile's repository itself cannot be resolved; fail closed
        // rather than guess that a resolvable worktree belongs to it.
        return WorktreeOwnership::Foreign;
    };
    if same_repository_path(&common_dir, profile_common_dir) {
        WorktreeOwnership::Owned
    } else {
        WorktreeOwnership::Foreign
    }
}

/// Returns the set of worktree directory names that correspond to branches
/// whose PR/MR is MERGED or CLOSED_UNMERGED. These worktrees are safe to prune
/// immediately (they are already terminal upstream), regardless of the
/// retention window. Best-effort: any fetch error yields an empty set so pruning
/// never blocks on transient provider/network failure.
fn done_worktree_names(profile: &Profile) -> HashSet<String> {
    match sync::fetch_mrs(profile) {
        Ok(mrs) => done_worktree_names_from_mrs(&mrs),
        Err(_) => HashSet::new(),
    }
}

/// Pure subset of `done_worktree_names`: maps a list of fetched PRs/MRs to the
/// set of on-disk worktree names that are safe to prune immediately.
pub(crate) fn done_worktree_names_from_mrs(mrs: &[SyncMr]) -> HashSet<String> {
    let mut done = HashSet::new();
    for mr in mrs {
        let class = sync::classify(mr);
        if class == "MERGED" || class == "CLOSED_UNMERGED" {
            // Worktree directory names are the branch name with '/' replaced by '-'
            // (see worktree::create / worktree::create_existing). The prune
            // filters (`worktree_name_matches_profile` plus the repository
            // ownership check) already scope results to this profile's own
            // `gah-{repo_id}-` / `gah-exp-{repo_id}-` worktrees, so the bare
            // transformed branch is enough.
            done.insert(worktree_name_for_branch(&mr.branch));
        }
    }
    done
}

fn prune_sessions(profile: &Profile, cutoff: SystemTime, dry_run: bool) -> Result<()> {
    let root = Path::new(&profile.artifact_root).join("sessions");
    if !root.exists() {
        println!("  sessions: none");
        return Ok(());
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !is_older_than(&path, cutoff) {
            continue;
        }
        if dry_run {
            println!("  would remove session {}", path.display());
        } else if path.is_dir() {
            fs::remove_dir_all(&path)?;
            println!("  removed session {}", path.display());
        } else {
            fs::remove_file(&path)?;
            println!("  removed file {}", path.display());
        }
    }
    Ok(())
}

fn prune_worktrees(
    cfg: &GahConfig,
    profile: &Profile,
    cutoff: SystemTime,
    dry_run: bool,
    done: &HashSet<String>,
    abandoned_open: &HashSet<String>,
    protected: &HashSet<String>,
) -> Result<()> {
    // An empty defaults.worktree_base stays empty in the config (#1366):
    // dispatch plans against the resolved default, so prune must sweep the
    // same base or GAH-owned worktrees survive their own lifecycle.
    let root = crate::config::effective_worktree_base(&cfg.defaults);
    if !root.exists() {
        println!("  worktrees: none");
        return Ok(());
    }
    // The directory match is exact on the profile segment (see
    // `worktree_name_matches_profile`): repo ids may contain '-' themselves
    // (`app` vs `app-web`), so a plain `gah-{repo_id}-` prefix would also
    // sweep another profile's worktrees and apply this profile's retention
    // to them (#1547). Names that pass the filter are still not trusted
    // blindly: `gah-exp-app-...` is a valid name for both profiles `app`
    // and `exp-app`, so every removal candidate must additionally resolve
    // to THIS profile's repository (`classify_worktree_ownership`).
    // Chat-session worktrees (apps/server chatSessions.ts) share the base
    // and the same age+clean rules: an idle-but-clean chat worktree is safe
    // to reclaim because the session's durable state (log + branch)
    // survives and the worktree rematerializes on resume; a dirty one is
    // retained for the session's own archive-with-patch step, never
    // force-removed.
    let profile_common_dir = rev_parse_absolute(Path::new(&profile.local_path), "--git-common-dir");
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !worktree_name_matches_profile(&name, &profile.repo_id) {
            continue;
        }
        if protected.contains(name.as_str()) {
            continue;
        }
        // A worktree tied to a merged/closed PR/MR is safe to prune
        // immediately, even if it is newer than the retention cutoff. Local
        // branch topology is deliberately not enough: a fresh worktree has a
        // default-branch tip until its agent makes the first commit, so using
        // that signal would race an in-flight manual dispatch.
        let is_done = done.contains(name.as_str());
        let is_abandoned_open = abandoned_open.contains(name.as_str());
        let is_old = is_older_than(&path, cutoff);
        if !is_done && !is_abandoned_open && !is_old {
            continue;
        }
        // Ownership check (#1547): a name-matched candidate that is a
        // linked worktree of a different repository is another profile's
        // worktree -- this profile's retention and the remove_dir_all
        // fallback must not touch it. Orphaned directories keep today's
        // handling below.
        if matches!(
            classify_worktree_ownership(&path, profile_common_dir.as_deref()),
            WorktreeOwnership::Foreign
        ) {
            println!(
                "  retained worktree {} (owned by another repository)",
                path.display()
            );
            continue;
        }
        // Never force-remove a dirty worktree. A terminal PR can still have
        // local, unpublished recovery work, and a failed worker can leave a
        // useful patch behind. This is intentionally fail-closed: if Git
        // cannot establish cleanliness, retain the worktree for inspection.
        if !worktree_is_clean(&path) {
            println!("  retained dirty worktree {}", path.display());
            continue;
        }
        if dry_run {
            println!(
                "  would remove worktree {}",
                if is_done {
                    format!("{} (MERGED/CLOSED)", path.display())
                } else if is_abandoned_open {
                    format!("{} (OPEN/ABANDONED)", path.display())
                } else {
                    path.display().to_string()
                }
            );
        } else {
            let git_result = crate::worktree::git(
                &["worktree", "remove", "-f", path.to_str().unwrap_or("")],
                Path::new(&profile.local_path),
            );
            // `git worktree remove` fails outright ("is not a working tree")
            // once git's own admin metadata for this worktree is already
            // gone -- e.g. left behind by a prior interrupted/partial
            // removal. Previously that failure was silently discarded and
            // "removed" printed anyway, so the directory (and its disk
            // usage) never actually went away despite being logged as
            // pruned on every run. Fall back to a direct removal whenever
            // the directory is still there, since `worktree_is_clean`
            // already confirmed above it's safe to delete.
            if path.exists() {
                if let Err(err) = fs::remove_dir_all(&path) {
                    println!(
                        "  failed to remove worktree {}: {}{}",
                        path.display(),
                        err,
                        git_result
                            .err()
                            .map(|e| format!(" (git worktree remove also failed: {e})"))
                            .unwrap_or_default()
                    );
                    continue;
                }
            }
            println!(
                "  removed worktree {}",
                if is_done {
                    format!("{} (MERGED/CLOSED)", path.display())
                } else if is_abandoned_open {
                    format!("{} (OPEN/ABANDONED)", path.display())
                } else {
                    path.display().to_string()
                }
            );
        }
    }
    if !dry_run {
        let _ = crate::worktree::git(&["worktree", "prune"], Path::new(&profile.local_path));
    }
    Ok(())
}

fn worktree_is_clean(worktree: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree)
        .output()
        .map(|out| out.status.success() && out.stdout.is_empty())
        .unwrap_or(false)
}

fn is_older_than(path: &PathBuf, cutoff: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|modified| modified <= cutoff)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        automatic_worktree_sets, done_worktree_names_from_mrs, is_older_than, prune_worktrees,
        remote_gah_branches_to_delete, worktree_name_for_branch, worktree_name_matches_profile,
    };
    use crate::config::{tests::test_profile_for_notifications, Defaults, GahConfig};
    use crate::sync::SyncMr;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    /// A realistic dispatch/identity.rs `timestamp()`:
    /// `"{unix seconds}-{uuid v4 simple}"`. Prune only considers
    /// profile-shaped names (#1547), so test fixtures must use the real
    /// dispatch shape, not shorthand like `gah-repo-orphaned`.
    const TEST_DISPATCH_TS: &str = "1730000000-0123456789abcdef0123456789abcdef";

    fn git(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} in {} failed: {}",
            args,
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A repository with one commit, so `git worktree add` can check out
    /// HEAD.
    fn init_repo_with_commit(path: &Path) {
        git(path, &["init", "--quiet", "-b", "main"]);
        git(
            path,
            &[
                "-c",
                "user.email=prune@test",
                "-c",
                "user.name=prune test",
                "commit",
                "--allow-empty",
                "--quiet",
                "-m",
                "init",
            ],
        );
    }

    /// A linked worktree of `repo` at `dir` -- the shape
    /// `worktree::create` produces: registered under the repository's
    /// `.git/worktrees/`, sharing its common git dir. Prune is
    /// directory-name driven, so the checked-out branch is irrelevant
    /// to these regressions and the worktree is detached.
    fn add_linked_worktree(repo: &Path, dir: &Path) {
        git(
            repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                dir.to_str().unwrap(),
            ],
        );
    }

    fn touch_old(path: &Path) {
        std::process::Command::new("touch")
            .args(["-t", "202401010000", path.to_str().unwrap()])
            .output()
            .unwrap();
    }

    fn config_with_worktree_base(worktree_base: &Path) -> GahConfig {
        GahConfig {
            defaults: Defaults {
                worktree_base: worktree_base.to_string_lossy().to_string(),
                ..Default::default()
            },
            profiles: HashMap::new(),
            context: Default::default(),
        }
    }

    fn profile_on_repo(repo_id: &str, local_path: &Path) -> crate::config::Profile {
        let mut profile = test_profile_for_notifications();
        profile.repo_id = repo_id.to_string();
        profile.local_path = local_path.to_string_lossy().to_string();
        profile
    }

    fn day_ago_cutoff() -> SystemTime {
        SystemTime::now()
            .checked_sub(Duration::from_secs(86_400))
            .unwrap()
    }

    /// Regression for the "removed worktree" log lying about success: a
    /// worktree directory whose git admin metadata is already gone (e.g.
    /// left behind by a prior interrupted removal) makes `git worktree
    /// remove -f` fail with "is not a working tree" every single run. The
    /// old code discarded that error and printed "removed" unconditionally,
    /// so the directory (and its disk usage) never actually went away.
    /// The orphan classification (`classify_worktree_ownership`) must keep
    /// this directory on today's handling, not retain it as foreign.
    #[test]
    fn prune_worktrees_falls_back_to_direct_removal_when_git_no_longer_tracks_it() {
        let worktree_base = tempfile::tempdir().unwrap();
        let stale = worktree_base
            .path()
            .join(format!("gah-repo-{TEST_DISPATCH_TS}"));
        fs::create_dir_all(&stale).unwrap();
        // Old enough to be eligible for pruning, and not registered as a
        // linked worktree of any repo -- reproducing the real orphaned state.
        git(&stale, &["init", "--quiet"]);
        touch_old(&stale);

        let local_path = tempfile::tempdir().unwrap();
        git(local_path.path(), &["init", "--quiet"]);

        let cfg = config_with_worktree_base(worktree_base.path());
        let profile = profile_on_repo("repo", local_path.path());

        prune_worktrees(
            &cfg,
            &profile,
            day_ago_cutoff(),
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();

        assert!(
            !stale.exists(),
            "orphaned worktree directory should be removed even when `git worktree remove` fails"
        );
    }

    #[test]
    fn older_than_uses_file_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let path = PathBuf::from(tmp.path()).join("old");
        fs::write(&path, "x").unwrap();
        std::process::Command::new("touch")
            .args(["-t", "202401010000", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(is_older_than(
            &path,
            SystemTime::now()
                .checked_sub(Duration::from_secs(86_400))
                .unwrap()
        ));
    }

    /// Chat-session worktrees share the worktree base and the gah-chat-<repo>-
    /// naming prefix (apps/server chatSessions.ts); the same age+clean prune
    /// that reclaims dispatch worktrees must reclaim idle chat ones too --
    /// and must never touch a directory that doesn't carry the prefix.
    #[test]
    fn prune_worktrees_reclaims_idle_chat_worktrees_and_ignores_foreign_dirs() {
        let worktree_base = tempfile::tempdir().unwrap();
        let chat = worktree_base.path().join("gah-chat-repo-abc123");
        let foreign = worktree_base.path().join("someone-elses-worktree");
        for dir in [&chat, &foreign] {
            fs::create_dir_all(dir).unwrap();
            git(dir, &["init", "--quiet"]);
            touch_old(dir);
        }

        let local_path = tempfile::tempdir().unwrap();
        git(local_path.path(), &["init", "--quiet"]);

        let cfg = config_with_worktree_base(worktree_base.path());
        let profile = profile_on_repo("repo", local_path.path());

        prune_worktrees(
            &cfg,
            &profile,
            day_ago_cutoff(),
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();

        assert!(!chat.exists(), "idle chat worktree should be reclaimed");
        assert!(
            foreign.exists(),
            "a directory without a gah prefix must never be touched"
        );
    }

    /// Regression for #1547: repo ids may contain '-' themselves (profiles
    /// `app` vs `app-web`), so the old plain `starts_with("gah-{repo_id}-")`
    /// sweep for `app` also matched -- and removed -- `app-web`'s worktrees
    /// in the shared base, applying `app`'s retention to another profile's
    /// work and ignoring `app-web`'s own live-claim protection. Dispatch
    /// and experiment names are separated by the exact profile segment plus
    /// the generator's leading `{unix seconds}-` digit run. The chat name
    /// `gah-chat-app-web-...` still name-matches profile `app` (session ids
    /// have no protective shape), so only the repository-ownership check
    /// keeps it safe -- hence two distinct repositories in this fixture.
    #[test]
    fn prune_worktrees_does_not_sweep_a_longer_repo_ids_worktrees() {
        let worktree_base = tempfile::tempdir().unwrap();
        let own = worktree_base
            .path()
            .join(format!("gah-app-{TEST_DISPATCH_TS}"));
        let foreign_dispatch = worktree_base
            .path()
            .join(format!("gah-app-web-{TEST_DISPATCH_TS}"));
        let foreign_experiment = worktree_base
            .path()
            .join(format!("gah-exp-app-web-{TEST_DISPATCH_TS}"));
        let foreign_chat = worktree_base.path().join("gah-chat-app-web-1a2b3c4d");

        let app_repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(app_repo.path());
        let app_web_repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(app_web_repo.path());

        add_linked_worktree(app_repo.path(), &own);
        for dir in [&foreign_dispatch, &foreign_experiment, &foreign_chat] {
            add_linked_worktree(app_web_repo.path(), dir);
        }
        for dir in [&own, &foreign_dispatch, &foreign_experiment, &foreign_chat] {
            touch_old(dir);
        }

        let app = profile_on_repo("app", app_repo.path());
        let app_web = profile_on_repo("app-web", app_web_repo.path());
        let mut cfg = config_with_worktree_base(worktree_base.path());
        cfg.profiles = HashMap::from([
            ("app".to_string(), app.clone()),
            ("app-web".to_string(), app_web),
        ]);

        prune_worktrees(
            &cfg,
            &app,
            day_ago_cutoff(),
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();

        assert!(
            !own.exists(),
            "app's own old, clean dispatch worktree should still be pruned"
        );
        for (kind, dir) in [
            ("dispatch", &foreign_dispatch),
            ("experiment", &foreign_experiment),
            ("chat", &foreign_chat),
        ] {
            assert!(
                dir.exists(),
                "app-web's {kind} worktree must survive a prune of app (#1547)"
            );
        }
    }

    /// #1547: `gah-exp-app-{seconds}-...` is a valid experiment name for
    /// profile `app` (via the `gah-exp-` prefix) and a valid dispatch name
    /// for profile `exp-app` (via `gah-`); the naming rules alone cannot
    /// separate the two. With the profiles on different repositories the
    /// mandatory ownership check can: pruning `app` must retain
    /// `exp-app`'s worktree, and the owning profile must still reclaim it.
    #[test]
    fn prune_worktrees_owns_colliding_exp_names_by_repository() {
        let worktree_base = tempfile::tempdir().unwrap();
        let colliding = worktree_base
            .path()
            .join(format!("gah-exp-app-{TEST_DISPATCH_TS}"));

        let app_repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(app_repo.path());
        let exp_app_repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(exp_app_repo.path());

        add_linked_worktree(exp_app_repo.path(), &colliding);
        touch_old(&colliding);

        let app = profile_on_repo("app", app_repo.path());
        let exp_app = profile_on_repo("exp-app", exp_app_repo.path());
        let mut cfg = config_with_worktree_base(worktree_base.path());
        cfg.profiles = HashMap::from([
            ("app".to_string(), app.clone()),
            ("exp-app".to_string(), exp_app.clone()),
        ]);

        prune_worktrees(
            &cfg,
            &app,
            day_ago_cutoff(),
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();
        assert!(
            colliding.exists(),
            "profile app must not apply its retention to exp-app's worktree just because the name collides (#1547)"
        );

        prune_worktrees(
            &cfg,
            &exp_app,
            day_ago_cutoff(),
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();
        assert!(
            !colliding.exists(),
            "the owning profile must still reclaim its own worktree"
        );
    }

    /// Production regression (#1547 review): central carries existing
    /// dispatch worktrees whose tails are not the current 32-char simple
    /// UUID, e.g. the exact name `gah-gah-1785867579-c6a81b07cce4497f496a04981ffd7`
    /// (29 hex chars). A 32-hex requirement silently disabled their
    /// cleanup; the name rule only requires the generator's
    /// `{unix seconds}-` segment, and the real worktree of this profile's
    /// repository must stay prunable.
    #[test]
    fn prune_worktrees_reclaims_production_worktrees_with_non_uuid_tails() {
        let worktree_base = tempfile::tempdir().unwrap();
        let production = worktree_base
            .path()
            .join("gah-gah-1785867579-c6a81b07cce4497f496a04981ffd7");

        let repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(repo.path());
        add_linked_worktree(repo.path(), &production);
        touch_old(&production);

        let cfg = config_with_worktree_base(worktree_base.path());
        let profile = profile_on_repo("gah", repo.path());

        prune_worktrees(
            &cfg,
            &profile,
            day_ago_cutoff(),
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();

        assert!(
            !production.exists(),
            "the exact production-shaped name observed on central must stay prunable"
        );
    }

    /// #1547: chat session ids may be any supplied `[a-zA-Z0-9_-]{1,128}`
    /// string (apps/server chatSessions.ts, e.g. `test-session`), so the
    /// chat name rule accepts any non-empty session and cannot separate
    /// profiles by shape at all. Ownership must carry that: this
    /// repository's chat worktree is reclaimed, a name-matching chat
    /// worktree of another repository is retained.
    #[test]
    fn prune_worktrees_owns_chat_worktrees_by_repository_not_session_shape() {
        let worktree_base = tempfile::tempdir().unwrap();
        let own = worktree_base.path().join("gah-chat-app-test-session");
        let foreign = worktree_base.path().join("gah-chat-app-coordinator-issued");

        let app_repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(app_repo.path());
        let other_repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(other_repo.path());

        add_linked_worktree(app_repo.path(), &own);
        add_linked_worktree(other_repo.path(), &foreign);
        touch_old(&own);
        touch_old(&foreign);

        let cfg = config_with_worktree_base(worktree_base.path());
        let profile = profile_on_repo("app", app_repo.path());

        prune_worktrees(
            &cfg,
            &profile,
            day_ago_cutoff(),
            false,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();

        assert!(
            !own.exists(),
            "a chat worktree of this repository with a custom dashed session id must be reclaimed"
        );
        assert!(
            foreign.exists(),
            "a name-matching chat worktree of another repository must be retained (#1547)"
        );
    }

    /// The pure core of the regressions above, per prefix: this profile's
    /// own shapes match, a longer repo id's dispatch/experiment names
    /// never do, and chat names -- whose session ids have no protective
    /// shape -- match every repo id the name could be split around,
    /// leaving cross-profile retention to the ownership check.
    #[test]
    fn worktree_match_is_exact_on_the_profile_segment() {
        assert!(worktree_name_matches_profile(
            &format!("gah-app-{TEST_DISPATCH_TS}"),
            "app"
        ));
        assert!(worktree_name_matches_profile(
            &format!("gah-exp-app-{TEST_DISPATCH_TS}"),
            "app"
        ));
        assert!(worktree_name_matches_profile(
            "gah-chat-app-1a2b3c4d",
            "app"
        ));
        assert!(worktree_name_matches_profile(
            "gah-chat-app-3f2a1c0d-9b8e-4c21-8d5a-2f1e0a9b7c6d",
            "app"
        ));
        // A supplied custom id (chatSessions.ts `[a-zA-Z0-9_-]{1,128}`).
        assert!(worktree_name_matches_profile(
            "gah-chat-app-test-session",
            "app"
        ));

        assert!(!worktree_name_matches_profile(
            &format!("gah-app-web-{TEST_DISPATCH_TS}"),
            "app"
        ));
        assert!(!worktree_name_matches_profile(
            &format!("gah-exp-app-web-{TEST_DISPATCH_TS}"),
            "app"
        ));

        // The chat segment cannot separate profiles: `web-1a2b3c4d` is a
        // valid non-empty session for `app` and the real session for
        // `app-web`. Retaining the foreign one is the ownership check's
        // job (prune_worktrees_does_not_sweep_a_longer_repo_ids_worktrees).
        assert!(worktree_name_matches_profile(
            "gah-chat-app-web-1a2b3c4d",
            "app"
        ));
        assert!(worktree_name_matches_profile(
            "gah-chat-app-web-1a2b3c4d",
            "app-web"
        ));
        // The exp-app/app collision the ownership check must carry:
        // `gah-exp-app-...` is `app`'s experiment and `exp-app`'s
        // dispatch name at once.
        assert!(worktree_name_matches_profile(
            &format!("gah-exp-app-{TEST_DISPATCH_TS}"),
            "exp-app"
        ));

        // No session segment at all is never a session.
        assert!(!worktree_name_matches_profile("gah-chat-app-", "app"));
        assert!(!worktree_name_matches_profile("gah-chat-app", "app"));

        assert!(worktree_name_matches_profile(
            &format!("gah-app-web-{TEST_DISPATCH_TS}"),
            "app-web"
        ));
        assert!(worktree_name_matches_profile(
            &format!("gah-exp-app-web-{TEST_DISPATCH_TS}"),
            "app-web"
        ));
        assert!(worktree_name_matches_profile(
            "gah-chat-app-web-1a2b3c4d",
            "app-web"
        ));
    }

    /// The dispatch suffix only has to start with the generator's
    /// `{unix seconds}-` segment; the tail after it is unconstrained
    /// because production worktrees carry 25, 29, 30 and 33 character
    /// hex tails. Without the leading digit run the name is another
    /// repo's (or a human's) directory and is retained.
    #[test]
    fn worktree_match_requires_the_dispatch_timestamp_segment() {
        for name in [
            format!("gah-app-{TEST_DISPATCH_TS}"),
            "gah-app-1730000000-deadbeef".to_string(),
            format!("gah-app-1730000000-{TEST_DISPATCH_TS}-extra"),
            // The exact production name observed on central (29-hex tail).
            "gah-gah-1785867579-c6a81b07cce4497f496a04981ffd7".to_string(),
            // The other observed tail lengths: 25, 30 and 33 hex chars.
            format!("gah-gah-1785867579-{}", "a".repeat(25)),
            format!("gah-gah-1785867579-{}", "b".repeat(30)),
            format!("gah-gah-1785867579-{}", "c".repeat(33)),
        ] {
            let repo_id = if name.starts_with("gah-gah-") {
                "gah"
            } else {
                "app"
            };
            assert!(
                worktree_name_matches_profile(&name, repo_id),
                "{name} must match profile {repo_id}"
            );
        }
        for name in [
            "gah-app-1730000000".to_string(),
            "gah-app-not-a-timestamp".to_string(),
            "gah-app-web-1730000000-x".to_string(),
        ] {
            assert!(
                !worktree_name_matches_profile(&name, "app"),
                "{name} must not match profile app"
            );
        }
    }

    #[test]
    fn worktree_name_mirrors_branch_slash_replacement() {
        assert_eq!(worktree_name_for_branch("gah/real-123"), "gah-real-123");
        assert_eq!(worktree_name_for_branch("feature/fix"), "feature-fix");
    }

    fn published_entry(branch: &str, work_id: &str) -> crate::ledger::LedgerEntry {
        let mut profile = test_profile_for_notifications();
        profile.repo_id = "real".into();
        let mut entry = crate::ledger::LedgerEntry::new(
            "real", &profile, "codex", "improve", work_id, None, None,
        );
        entry.work_id = Some(work_id.into());
        entry.branch = Some(branch.into());
        entry.mr_created = true;
        entry
    }

    fn active_mr(branch: &str, work_id: &str) -> crate::sync::SyncMrJson {
        let mut mr = mr(branch, false, Some("open"));
        mr.work_id = Some(work_id.into());
        crate::sync::sync_mr_to_json(&mr, None, &Default::default())
    }

    #[test]
    fn automatic_sets_protect_every_published_worktree_for_a_live_claim() {
        let entries = vec![
            published_entry("gah/real-prior", "#915"),
            published_entry("gah/real-active", "TICKET-915"),
        ];
        let mrs = vec![active_mr("gah/real-active", "#915")];
        let claims = vec![crate::status::ActiveClaimSnapshot {
            work_id: "#915".into(),
            pid: std::process::id(),
            scope: "real@real".into(),
            hostname: "test".into(),
            claimed_at: "2099-01-01T00:00:00Z".into(),
            age_seconds: 0,
            route: None,
        }];

        let sets = automatic_worktree_sets(&entries, "real", "real", Some(&mrs), Some(&claims));

        assert_eq!(
            sets.protected,
            HashSet::from(["gah-real-active".into(), "gah-real-prior".into()])
        );
        assert!(sets.done.contains("gah-real-prior"));
        assert!(sets.abandoned_open.is_empty());
    }

    #[test]
    fn automatic_sets_do_not_infer_state_from_an_incomplete_mr_observation() {
        let entries = vec![published_entry("gah/real-915", "#915")];

        let sets = automatic_worktree_sets(&entries, "real", "real", None, Some(&[]));

        assert!(sets.done.is_empty());
        assert!(sets.abandoned_open.is_empty());
    }

    #[test]
    fn automatic_sets_fail_closed_when_claim_observation_is_incomplete() {
        let entries = vec![published_entry("gah/real-915", "#915")];
        let mrs = vec![active_mr("gah/real-915", "#915")];

        let sets = automatic_worktree_sets(&entries, "real", "real", Some(&mrs), None);

        assert_eq!(sets.protected, HashSet::from(["gah-real-915".into()]));
        assert!(sets.abandoned_open.is_empty());
    }

    #[test]
    fn automatic_sets_never_classify_the_manager_chat_namespace() {
        let entries = vec![published_entry("gah/chat/real-session", "#915")];
        let mrs = vec![active_mr("gah/chat/real-session", "#915")];

        let sets = automatic_worktree_sets(&entries, "real", "real", Some(&mrs), None);

        assert!(sets.done.is_empty());
        assert!(sets.abandoned_open.is_empty());
        assert!(sets.protected.is_empty());
    }

    fn mr(branch: &str, merged: bool, state: Option<&str>) -> SyncMr {
        SyncMr {
            title: String::new(),
            body: None,
            branch: branch.to_string(),
            labels: Vec::new(),
            url: None,
            id: None,
            state: state.map(|s| s.to_string()),
            draft: false,
            source_sha: None,
            merge_commit_sha: None,
            merge_status: None,
            merged,
            updated_at: None,
            merged_at: None,
            ci_failed: false,
            ci_passed: false,
            ci_pending: false,
            work_id: None,
        }
    }

    #[test]
    fn done_set_covers_merged_and_closed_unmerged_only() {
        let mrs = vec![
            mr("gah/real-1", true, None),            // MERGED
            mr("gah/real-2", false, Some("closed")), // CLOSED_UNMERGED
            mr("gah/real-3", false, Some("open")),   // open -> not done
            mr("gah/real-4", false, None),           // open -> not done
        ];
        let done = done_worktree_names_from_mrs(&mrs);
        assert!(done.contains("gah-real-1"));
        assert!(done.contains("gah-real-2"));
        assert!(!done.contains("gah-real-3"));
        assert!(!done.contains("gah-real-4"));
    }

    #[test]
    fn remote_delete_covers_merged_and_closed_unmerged_gah_branches_only() {
        let mrs = vec![
            mr("gah/real-1", true, None),            // MERGED, gah/ -> delete
            mr("gah/real-2", false, Some("closed")), // CLOSED_UNMERGED, gah/ -> delete
            mr("gah/real-3", false, Some("open")),   // open, gah/ -> not terminal
            mr("gah/real-4", false, None),           // open, gah/ -> not terminal
        ];
        let to_delete = remote_gah_branches_to_delete(&mrs);
        assert_eq!(
            to_delete,
            vec![("gah/real-1", "MERGED"), ("gah/real-2", "CLOSED_UNMERGED")]
        );
    }

    #[test]
    fn remote_delete_never_touches_a_human_authored_branch_even_when_merged() {
        // A human's own merged PR must never be auto-deleted just because
        // it shares a repo with gah's dispatch branches -- only the exact
        // `gah/` naming convention is eligible.
        let mrs = vec![
            mr("feat/my-manual-work", true, None),
            mr("chore/cleanup", false, Some("closed")),
            mr("fix/something", true, None),
        ];
        assert!(remote_gah_branches_to_delete(&mrs).is_empty());
    }

    #[test]
    fn remote_delete_prefix_check_is_exact_not_substring() {
        // "not-gah/foo" contains "gah/" as a substring but does not START
        // with it -- must not match.
        let mrs = vec![mr("not-gah/foo", true, None)];
        assert!(remote_gah_branches_to_delete(&mrs).is_empty());
    }
}
