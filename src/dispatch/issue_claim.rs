//! Issue and pull request claims that every loop working a repository can
//! see.
//!
//! With `issue_claim.mode = "github_assignee"` a loop claims an item by
//! assigning its own GitHub login and posting a claim comment. Implement work
//! claims its issue; review and fix work claim the pull request they publish
//! to (see [`ClaimTarget`]). GitHub keeps the assignees and comments of a
//! pull request on the issue of the same number, so both kinds use the same
//! calls and the same rules. A login holds an item only while both are
//! present: the assignee proves the login may work in the repository, and
//! the comment gives the claim a time and an order. Author and time are read
//! from the comment's own metadata, never from its text, so a comment cannot
//! claim on behalf of someone else.
//!
//! The rules, all decided by [`standing`] ("issue" below means either kind,
//! except where a rule names one):
//!
//! - A claim is live for `ttl_minutes` after its comment was posted or last
//!   renewed. A dispatch that won a claim renews it while it runs (see
//!   [`IssueLease`]), so only an abandoned claim expires. A dispatch whose
//!   lease is lost stops before it publishes. The lease ends at the claim's
//!   confirmed expiry unless a renewal is confirmed first, however long a
//!   renewal call takes, and each renewal call is cut off at that expiry.
//! - An assignee with no claim comment, other than this loop's own login,
//!   was assigned by hand: a person, not a loop (this loop's own login
//!   assigned by hand asks this loop to do the work). A person holds the
//!   issue against every loop, whatever else is assigned, until the person
//!   is removed. No loop ever removes or replaces such an assignee: a claim
//!   only adds the loop's own login, and a loop only removes its own login
//!   or another loop's expired one.
//! - When two loops hold live claims, the first login in `priority_logins`
//!   wins, otherwise the earliest claim comment.
//! - An expired claim on an issue keeps holding the issue while an open pull
//!   request references it. Otherwise another loop may remove the stale
//!   assignee and claim the issue itself.
//! - An expired claim on a pull request holds nothing. An open pull request
//!   is the normal state of one under review or repair, so it cannot be what
//!   keeps a lapsed claim: only renewal, the holder's proof of life, can.
//!   Another loop may remove the stale assignee and claim it at once.
//!
//! Work with no GitHub issue (a ticket file, a candidates file, a target
//! that is not an issue number) has nothing to claim on the provider and
//! runs under the machine-local claim alone; see [`provider_claimed_issue`].
//! Review and fix work publishes to a pull request; when that pull request
//! is unknown it fails closed instead (see [`IssueClaimSlot::for_review`]
//! and [`IssueClaimSlot::for_improve`]).
//!
//! The machine-local claim store still guards against two workers on one
//! machine; this module only arbitrates between logins.
//!
//! A dispatch claims its issue only once it holds a backend and node slot
//! (see [`IssueClaimSlot::admit`]), so a dispatch that is refused capacity never
//! announces work it will not do.

use super::attempts::{reserve_backend_attempt, BackendAdmissionGuard};
use super::DispatchArgs;
use crate::clock::Deadline;
use crate::config::{IssueClaimMode, IssueClaimPolicy, Profile};
use crate::execution_identity::ExecutionIdentity;
use crate::ledger::LedgerEntry;
use crate::provider::provider_command;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// First characters of every claim comment. The rest of the first line is
/// for people reading the raw comment; no decision reads it.
const CLAIM_MARKER: &str = "<!-- gah-claim ";

/// Result of trying to claim an issue for this loop.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ClaimOutcome {
    /// This loop holds the issue and may dispatch it until `expires_at`, the
    /// expiry of its claim as the provider recorded it, unless it renews.
    Won { expires_at: OffsetDateTime },
    /// Someone else holds the issue. The text says who and why.
    Lost(String),
}

/// The provider item a dispatch claims, by its number as bare digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClaimTarget {
    /// The issue implement work is for.
    Issue(String),
    /// The pull request review or fix work publishes to.
    PullRequest(String),
}

impl ClaimTarget {
    fn number(&self) -> &str {
        match self {
            Self::Issue(number) | Self::PullRequest(number) => number,
        }
    }
}

impl std::fmt::Display for ClaimTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Issue(number) => write!(formatter, "issue #{number}"),
            Self::PullRequest(number) => write!(formatter, "pull request #{number}"),
        }
    }
}

/// A dispatch ended because another login holds its issue or pull request.
/// Not a failure: the controller reports it as a skip and the ledger does not
/// count it as an attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssueClaimLost {
    target: ClaimTarget,
    reason: String,
}

impl std::fmt::Display for IssueClaimLost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.target, self.reason)
    }
}

impl std::error::Error for IssueClaimLost {}

pub(crate) fn issue_claim_lost(error: &anyhow::Error) -> Option<&IssueClaimLost> {
    error.chain().find_map(|cause| cause.downcast_ref())
}

/// One dispatch's provider claim on its issue or pull request, from its
/// first admitted attempt until the workflow returns. Claims nothing for a
/// profile that keeps claims local.
#[derive(Debug, Default)]
pub(super) struct IssueClaimSlot {
    /// What the dispatch claims once it is admitted; `None` claims nothing.
    target: Option<ClaimTarget>,
    lease: Option<IssueLease>,
}

impl IssueClaimSlot {
    /// The slot of an implement or fix dispatch of `target`. Work on an
    /// existing branch publishes to that branch's pull request, so it claims
    /// the pull request, and fails if the pull request cannot be found. Other
    /// work claims `target`'s issue when it names one.
    pub(super) fn for_improve(
        profile: &Profile,
        target: &str,
        existing_branch: Option<&str>,
    ) -> Result<Self> {
        if profile.publishing.issue_claim.mode == IssueClaimMode::Local {
            return Ok(Self::default());
        }
        let Some(branch) = existing_branch else {
            return Ok(Self {
                target: provider_claimed_issue(profile, target).map(ClaimTarget::Issue),
                lease: None,
            });
        };
        GithubBoard::for_reading(profile)?;
        let number = crate::provider::github_find_pr_number_by_branch(profile, branch)
            .with_context(|| format!("finding the pull request of {branch} to claim"))?;
        Self::for_review(profile, Some(&number))
    }

    /// The slot of a review of `pull_request`. A review publishes on its pull
    /// request, so without one there is nothing to claim and it fails closed
    /// instead of running unclaimed.
    pub(super) fn for_review(profile: &Profile, pull_request: Option<&str>) -> Result<Self> {
        if profile.publishing.issue_claim.mode == IssueClaimMode::Local {
            return Ok(Self::default());
        }
        let Some(number) = pull_request.filter(|number| is_issue_number(number)) else {
            anyhow::bail!(
                "issue_claim.mode = \"github_assignee\" claims the pull request this work \
                 publishes to, and none is known ({pull_request:?})"
            );
        };
        Ok(Self {
            target: Some(ClaimTarget::PullRequest(number.to_string())),
            lease: None,
        })
    }

    /// Reserve the backend and node slot for one attempt and, on a dispatch's
    /// first admitted attempt, claim its item and start renewing the claim.
    /// Later attempts first confirm the lease is still held. A refused slot
    /// fails as a capacity deferral. Another login holding the item fails
    /// with [`IssueClaimLost`]; the slot is released as that error returns.
    pub(super) fn admit(
        &mut self,
        profile: &Profile,
        identity: &ExecutionIdentity,
        args: &DispatchArgs,
        attempt: u32,
        ledger: &mut LedgerEntry,
    ) -> Result<BackendAdmissionGuard> {
        self.ensure_held()?;
        let guard = reserve_backend_attempt(profile, identity, args.route_admission.as_ref())
            .map_err(|error| super::contextualize_capacity_deferral(error, attempt as usize))?;
        if let (None, Some(target)) = (&self.lease, &self.target) {
            self.lease = Some(claim_or_lose(profile, target, ledger)?);
        }
        Ok(guard)
    }

    /// [`Self::admit`] for a dispatch working in `worktree`. A lost claim
    /// keeps the work so far on a local WIP commit and removes the worktree,
    /// as [`Self::ensure_held_before_publish`] does, so the worktree does not
    /// hold the branch against the loop's next dispatch of it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn admit_in(
        &mut self,
        worktree: &Path,
        repo: &Path,
        profile: &Profile,
        identity: &ExecutionIdentity,
        args: &DispatchArgs,
        attempt: u32,
        ledger: &mut LedgerEntry,
    ) -> Result<BackendAdmissionGuard> {
        let admitted = self.admit(profile, identity, args, attempt, ledger);
        match admitted {
            Err(lost) if issue_claim_lost(&lost).is_some() => {
                Err(keep_lost_work(lost, profile, &args.mode, worktree, repo))
            }
            admitted => admitted,
        }
    }

    /// The last check before a dispatch publishes. If another loop took the
    /// item over while this one worked, keep the work on a local WIP commit,
    /// remove the worktree, and fail with [`IssueClaimLost`]: nothing is
    /// published.
    pub(super) fn ensure_held_before_publish(
        &self,
        profile: &Profile,
        mode: &str,
        worktree: &Path,
        repo: &Path,
    ) -> Result<()> {
        self.ensure_held()
            .map_err(|lost| keep_lost_work(lost, profile, mode, worktree, repo))
    }

    /// Fails with [`IssueClaimLost`] once the dispatch's claim is lost or
    /// lapsed. The last check before a publish that has no worktree to keep,
    /// such as a review comment.
    pub(super) fn ensure_held(&self) -> Result<()> {
        self.lease.as_ref().map_or(Ok(()), IssueLease::ensure_held)
    }
}

/// Keep a dispatch's work on a local WIP commit and remove its worktree,
/// because its claim was `lost`. Returns `lost`, or the error that kept the
/// work from being saved.
fn keep_lost_work(
    lost: anyhow::Error,
    profile: &Profile,
    mode: &str,
    worktree: &Path,
    repo: &Path,
) -> anyhow::Error {
    if let Err(error) = crate::worktree::preserve_wip(
        worktree,
        &profile.default_target_branch,
        &format!("gah: WIP claim lost {mode}"),
    ) {
        return error;
    }
    crate::worktree::cleanup(worktree, repo);
    lost
}

/// The provider issue a dispatch of `target` must claim, as bare digits.
/// `None` when the profile keeps claims local, or when `target` names no
/// GitHub issue (a ticket file, a candidates file, an empty target): such
/// work has nothing on the provider for another loop to see, so it runs
/// under the machine-local claim alone. A `#123` target is the same issue as
/// `123`, which the dispatch resolves it to, so it is claimed the same way.
fn provider_claimed_issue(profile: &Profile, target: &str) -> Option<String> {
    if profile.publishing.issue_claim.mode == IssueClaimMode::Local
        || !super::issues::is_issue_number_reference(target)
    {
        return None;
    }
    super::issues::extract_issue_number(target)
}

/// Claim `target` on the provider, waiting out the verify window, and start
/// renewing the claim if this loop won it.
fn claim_or_lose(
    profile: &Profile,
    target: &ClaimTarget,
    ledger: &mut LedgerEntry,
) -> Result<IssueLease> {
    let policy = &profile.publishing.issue_claim;
    let board = GithubBoard::for_writing(profile)?;
    let me = own_login()?;
    let outcome = run_claim(&board, policy, &me, target, crate::clock::now_utc(), || {
        std::thread::sleep(Duration::from_secs(policy.verify_seconds.into()));
        crate::clock::now_utc()
    })?;
    match outcome {
        ClaimOutcome::Lost(reason) => {
            ledger.validation_result = Some(crate::ledger::gates::CLAIM_LOST.into());
            Err(IssueClaimLost {
                target: target.clone(),
                reason,
            }
            .into())
        }
        ClaimOutcome::Won { expires_at } => Ok(IssueLease::start(
            profile,
            target,
            me,
            Deadline::at(expires_at),
            claim_ttl(policy),
        )),
    }
}

fn claim_ttl(policy: &IssueClaimPolicy) -> Duration {
    Duration::from_secs(u64::from(policy.ttl_minutes) * 60)
}

/// A won claim, renewed in the background while the dispatch runs. Renewal
/// is the dispatch's proof of life: a loop that crashes stops renewing, and
/// its claim lapses `ttl_minutes` later. Dropping the lease stops renewal and
/// leaves the claim in place to lapse on its own (an issue's claim stays
/// held once a pull request for the issue is open).
///
/// The lease's authority ends at its deadline: the claim's confirmed expiry
/// on the provider, using a monotonic budget or active simulated expiry.
/// [`IssueLease::ensure_held`]
/// enforces it by itself, so a renewal that hangs or fails cannot keep a
/// dispatch authorized past the point where another loop may take the issue.
#[derive(Debug)]
pub(super) struct IssueLease {
    target: ClaimTarget,
    state: Arc<Mutex<LeaseState>>,
    stop: Option<mpsc::Sender<()>>,
    renewer: Option<std::thread::JoinHandle<()>>,
}

#[derive(Debug)]
struct LeaseState {
    /// Why the lease is lost, once it is. Never cleared.
    lost: Option<LeaseLoss>,
    /// When the claim expires unless a renewal is confirmed first. A renewal
    /// moves it to a TTL after the renewal began, which is no later than the
    /// time the provider records for it.
    deadline: Deadline,
}

#[derive(Debug, Clone)]
enum LeaseLoss {
    Claim(String),
    Policy(crate::policy::PolicyBlocked),
}

impl IssueLease {
    fn start(
        profile: &Profile,
        target: &ClaimTarget,
        me: String,
        deadline: Deadline,
        ttl: Duration,
    ) -> Self {
        #[cfg(test)]
        let probe = tests::RENEWAL_PROBE.with(|probe| probe.borrow_mut().take());
        #[cfg(test)]
        let clock = crate::clock::test_clock_handle();
        let state = Arc::new(Mutex::new(LeaseState {
            lost: None,
            deadline,
        }));
        let (stop, stopped) = mpsc::channel::<()>();
        let profile = profile.clone();
        let claimed = target.clone();
        let thread_state = Arc::clone(&state);
        let renewer = std::thread::spawn(move || {
            #[cfg(test)]
            let _clock_scope = clock.as_ref().map(|clock| clock.install());
            let now = || {
                #[cfg(test)]
                if let Some(probe) = &probe {
                    return *probe.now.lock().unwrap();
                }
                Instant::now()
            };
            let policy = profile.publishing.issue_claim.clone();
            let mut failed = false;
            loop {
                let deadline = lock(&thread_state).deadline;
                let wait = next_renewal_wait(deadline.remaining_at(now()), ttl, failed);
                let renewal_deadline = Deadline::after_at(wait, now());
                loop {
                    let remaining = renewal_deadline.remaining_at(now());
                    // Only simulated clocks need polling for advances. Production
                    // keeps the original single interruptible monotonic wait.
                    let wait = if crate::clock::override_now().is_some() {
                        remaining.min(Duration::from_millis(25))
                    } else {
                        remaining
                    };
                    if !matches!(stopped.recv_timeout(wait), Err(RecvTimeoutError::Timeout)) {
                        return;
                    }
                    if renewal_deadline.reached_at(now()) {
                        break;
                    }
                }
                let began = now();
                if deadline.reached_at(began) {
                    lose(
                        &thread_state,
                        "the claim expired before a renewal was confirmed",
                    );
                    return;
                }
                // Every provider call of the renewal must finish before the
                // claim it renews expires; past that, it proves nothing.
                let board = GithubBoard {
                    profile: &profile,
                    deadline: Some(deadline),
                };
                let extended = Deadline::after_at(ttl, began);
                match renew(&board, &policy, &me, &claimed, crate::clock::now_utc()) {
                    Ok(None) => {
                        failed = false;
                        if !confirm_renewal(&thread_state, extended, now()) {
                            return;
                        }
                        #[cfg(test)]
                        if let Some(probe) = &probe {
                            let _ = probe.confirmed.send(());
                        }
                    }
                    Ok(Some(reason)) => {
                        lose(&thread_state, &reason);
                        return;
                    }
                    Err(error) => {
                        if let Some(blocked) = crate::policy::policy_blocked(&error) {
                            lock(&thread_state).lost = Some(LeaseLoss::Policy(blocked.clone()));
                            eprintln!(
                                "warning: stopped renewing the claim on {claimed}: {error:#}"
                            );
                            return;
                        }
                        failed = true;
                        eprintln!(
                            "warning: could not renew the claim on {claimed}; \
                             retrying before it expires: {error:#}"
                        );
                    }
                }
            }
        });
        Self {
            target: target.clone(),
            state,
            stop: Some(stop),
            renewer: Some(renewer),
        }
    }

    /// Fails with [`IssueClaimLost`] once another loop has taken the item,
    /// this loop's claim was removed, or the claim's confirmed expiry has
    /// passed without a confirmed renewal, whatever the renewer is doing.
    /// The dispatch must stop: its work is no longer the work of record for
    /// the item.
    fn ensure_held(&self) -> Result<()> {
        let mut state = lock(&self.state);
        if state.lost.is_none() && state.deadline.reached() {
            state.lost = Some(LeaseLoss::Claim(
                "the claim expired before a renewal was confirmed".to_string(),
            ));
        }
        match state.lost.clone() {
            Some(LeaseLoss::Policy(error)) => Err(error.into()),
            Some(LeaseLoss::Claim(reason)) => Err(IssueClaimLost {
                target: self.target.clone(),
                reason,
            }
            .into()),
            None => Ok(()),
        }
    }
}

fn lock(state: &Mutex<LeaseState>) -> std::sync::MutexGuard<'_, LeaseState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn lose(state: &Mutex<LeaseState>, reason: &str) {
    lock(state)
        .lost
        .get_or_insert_with(|| LeaseLoss::Claim(reason.to_string()));
}

/// Record a renewal that succeeded, observed at `now`: move the deadline to
/// `extended`, unless the lease already lapsed. A renewal seen at or after
/// the current deadline proves nothing: the claim was expired in between,
/// when another loop could have taken the issue, so the lease is lost even
/// though the call succeeded and even if nobody checked in that gap.
/// Returns whether the lease still holds.
fn confirm_renewal(state: &Mutex<LeaseState>, extended: Deadline, now: Instant) -> bool {
    let mut state = lock(state);
    if state.lost.is_none() && state.deadline.reached_at(now) {
        state.lost = Some(LeaseLoss::Claim(
            "the claim expired before its renewal was confirmed".to_string(),
        ));
    }
    if state.lost.is_some() {
        return false;
    }
    state.deadline = extended;
    true
}

impl Drop for IssueLease {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(renewer) = self.renewer.take() {
            let _ = renewer.join();
        }
    }
}

/// How long the renewer waits before its next attempt, given the authority
/// `remaining` before the claim expires. A healthy lease renews once a third
/// of its TTL has passed since the claim was last confirmed, so an inherited
/// claim with little time left is renewed at once. After a failed attempt it
/// retries once half the remaining time has passed (at least a second apart),
/// so transient failures are retried while the claim is still live.
fn next_renewal_wait(remaining: Duration, ttl: Duration, after_failure: bool) -> Duration {
    if after_failure {
        (remaining / 2).max(Duration::from_secs(1)).min(remaining)
    } else {
        remaining.saturating_sub(ttl * 2 / 3)
    }
}

/// Extend this loop's live claim on `target`. Returns why the claim is lost
/// when this loop no longer holds the item, and renews nothing then.
fn renew(
    board: &impl IssueBoard,
    policy: &IssueClaimPolicy,
    me: &str,
    target: &ClaimTarget,
    now: OffsetDateTime,
) -> Result<Option<String>> {
    let view = board.view(target.number())?;
    match standing(&view, me, policy, now) {
        Standing::Mine { .. } => {}
        Standing::Theirs(login) => return Ok(Some(format!("lease taken over by {login}"))),
        Standing::Unclaimed | Standing::Stale(_) => {
            return Ok(Some(
                "this loop's claim lapsed or was removed while it worked".to_string(),
            ))
        }
    }
    let own = view
        .claims
        .iter()
        .filter(|claim| same_login(&claim.login, me))
        .max_by_key(|claim| claim.id)
        .context("a held claim has a claim comment")?;
    board.renew_claim(
        own.id,
        &claim_comment(me, policy, target, own.created_at, now),
    )?;
    Ok(None)
}

/// The claim protocol: [`stake`] at `now`, `pause` for the verify window,
/// then [`settle`] at the time the pause returns. The pause is not an
/// optimization: it is what lets a contender whose write was already in
/// flight become visible before this loop decides it won.
fn run_claim(
    board: &impl IssueBoard,
    policy: &IssueClaimPolicy,
    me: &str,
    target: &ClaimTarget,
    now: OffsetDateTime,
    pause: impl FnOnce() -> OffsetDateTime,
) -> Result<ClaimOutcome> {
    if let Some(outcome) = stake(board, policy, me, target, now)? {
        return Ok(outcome);
    }
    settle(board, policy, me, target, pause())
}

/// Numbers of the issues another login holds, out of `issues` given as
/// `(number, assignees)`. Intake uses this to leave those issues alone.
///
/// An issue whose claim comments cannot be read counts as held. Returns an
/// empty set without touching the provider when the profile keeps claims
/// local.
pub(super) fn issues_held_by_others<'a>(
    profile: &Profile,
    issues: impl IntoIterator<Item = (&'a str, &'a [String])>,
) -> Result<HashSet<String>> {
    let policy = &profile.publishing.issue_claim;
    if policy.mode == IssueClaimMode::Local {
        return Ok(HashSet::new());
    }
    let board = GithubBoard::for_reading(profile)?;
    let me = own_login()?;
    let now = crate::clock::now_utc();
    let mut held = HashSet::new();
    for (number, assignees) in issues {
        if assignees.iter().all(|login| same_login(login, &me)) {
            continue;
        }
        let view = match board.claims(number) {
            Ok(claims) => IssueClaimView {
                assignees: assignees.to_vec(),
                claims,
            },
            Err(error) => {
                eprintln!("warning: treating issue #{number} as claimed elsewhere: {error:#}");
                held.insert(number.to_string());
                continue;
            }
        };
        if matches!(standing(&view, &me, policy, now), Standing::Theirs(_)) {
            held.insert(number.to_string());
        }
    }
    Ok(held)
}

/// One claim comment, as GitHub recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaimComment {
    id: u64,
    login: String,
    created_at: OffsetDateTime,
    /// When the claim was last renewed: GitHub's `updated_at`, which moves
    /// only when the comment is edited. Equals `created_at` until then.
    renewed_at: OffsetDateTime,
}

/// The provider facts a claim decision reads.
#[derive(Debug, Clone, Default)]
struct IssueClaimView {
    assignees: Vec<String>,
    claims: Vec<ClaimComment>,
}

/// Who holds an issue, from one loop's point of view.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Standing {
    /// Nobody holds it.
    Unclaimed,
    /// This loop holds a live claim and wins against every other one, until
    /// `expires_at` unless it renews: a TTL after the claim's last renewal,
    /// never a TTL from now.
    Mine { expires_at: OffsetDateTime },
    /// This login holds it: assigned by hand, or with the winning live claim.
    Theirs(String),
    /// Only these other logins are assigned, and their claims have expired.
    /// The issue is free once no open pull request references it.
    Stale(Vec<String>),
}

fn standing(
    view: &IssueClaimView,
    me: &str,
    policy: &IssueClaimPolicy,
    now: OffsetDateTime,
) -> Standing {
    let ttl = time::Duration::minutes(policy.ttl_minutes.into());
    let mut live: Vec<(&str, &ClaimComment)> = Vec::new();
    let mut stale = Vec::new();
    for login in &view.assignees {
        let latest_claim = view
            .claims
            .iter()
            .filter(|claim| same_login(&claim.login, login))
            .max_by_key(|claim| claim.id);
        let mine = same_login(login, me);
        match latest_claim {
            Some(claim) if now - claim.renewed_at < ttl => live.push((login, claim)),
            Some(_) if !mine => stale.push(login.clone()),
            None if !mine => return Standing::Theirs(login.clone()),
            // This loop's own expired claim or hand assignment holds nothing
            // against itself; it claims afresh.
            _ => {}
        }
    }
    let winner = policy
        .priority_logins
        .iter()
        .find_map(|preferred| live.iter().find(|(login, _)| same_login(login, preferred)))
        .or_else(|| live.iter().min_by_key(|(_, claim)| claim.id));
    match winner {
        Some((login, claim)) if same_login(login, me) => Standing::Mine {
            expires_at: claim.renewed_at + ttl,
        },
        Some((login, _)) => Standing::Theirs(login.to_string()),
        None if stale.is_empty() => Standing::Unclaimed,
        None => Standing::Stale(stale),
    }
}

/// The provider operations the claim protocol performs. A seam so two loops
/// can race over one in-memory issue in tests.
trait IssueBoard {
    fn view(&self, issue: &str) -> Result<IssueClaimView>;
    fn assign(&self, issue: &str, login: &str) -> Result<()>;
    fn unassign(&self, issue: &str, logins: &[String]) -> Result<()>;
    fn post_claim(&self, issue: &str, body: &str) -> Result<()>;
    fn renew_claim(&self, comment_id: u64, body: &str) -> Result<()>;
    fn has_open_pull_request(&self, issue: &str) -> Result<bool>;
}

/// First half of a claim: decide from the item as it stands, and if it is
/// free, assign this loop and post its claim comment. `None` means the claim
/// was posted and must be confirmed by [`settle`] after the verify window.
fn stake(
    board: &impl IssueBoard,
    policy: &IssueClaimPolicy,
    me: &str,
    target: &ClaimTarget,
    now: OffsetDateTime,
) -> Result<Option<ClaimOutcome>> {
    let issue = target.number();
    match standing(&board.view(issue)?, me, policy, now) {
        Standing::Mine { expires_at } => return Ok(Some(ClaimOutcome::Won { expires_at })),
        Standing::Theirs(login) => {
            return Ok(Some(ClaimOutcome::Lost(format!("held by {login}"))));
        }
        Standing::Stale(logins) => {
            // Only an issue's expired claim can still hold: its open pull
            // request carries the work on. A pull request's is simply free.
            if matches!(target, ClaimTarget::Issue(_)) && board.has_open_pull_request(issue)? {
                return Ok(Some(ClaimOutcome::Lost(format!(
                    "the expired claim of {} still has an open pull request",
                    logins.join(", ")
                ))));
            }
            board.unassign(issue, &logins)?;
        }
        Standing::Unclaimed => {}
    }
    board.assign(issue, me)?;
    if let Err(error) = board.post_claim(issue, &claim_comment(me, policy, target, now, now)) {
        // An assignee without a claim comment reads as a hand assignment and
        // would hold the issue against every loop indefinitely.
        if let Err(cleanup) = board.unassign(issue, &[me.to_string()]) {
            eprintln!("warning: could not remove {me} from {target}: {cleanup:#}");
        }
        return Err(error);
    }
    Ok(None)
}

/// Second half of a claim: re-read the item after the verify window. A loop
/// that did not win removes its own assignee and posts nothing further.
fn settle(
    board: &impl IssueBoard,
    policy: &IssueClaimPolicy,
    me: &str,
    target: &ClaimTarget,
    now: OffsetDateTime,
) -> Result<ClaimOutcome> {
    let issue = target.number();
    let reason = match standing(&board.view(issue)?, me, policy, now) {
        Standing::Mine { expires_at } => return Ok(ClaimOutcome::Won { expires_at }),
        Standing::Theirs(login) => format!("yielded to {login}"),
        Standing::Unclaimed | Standing::Stale(_) => {
            "this loop's claim was removed during the verify window".to_string()
        }
    };
    board.unassign(issue, &[me.to_string()])?;
    Ok(ClaimOutcome::Lost(reason))
}

fn claim_comment(
    me: &str,
    policy: &IssueClaimPolicy,
    target: &ClaimTarget,
    claimed_at: OffsetDateTime,
    now: OffsetDateTime,
) -> String {
    let expires = now + time::Duration::minutes(policy.ttl_minutes.into());
    let stamp = |at: OffsetDateTime| {
        let at = at.replace_nanosecond(0).unwrap_or(at);
        at.format(&Rfc3339).unwrap_or_else(|_| at.to_string())
    };
    let node = hostname::get()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".to_string());
    let after_lapse = match target {
        ClaimTarget::Issue(_) => {
            "another loop may take this issue over, unless an open pull request references it"
        }
        ClaimTarget::PullRequest(_) => "another loop may take this pull request over",
    };
    // The timestamps also make each claim's text distinct, which
    // `post_issue_comment` needs: it never repeats an identical comment.
    format!(
        "{CLAIM_MARKER}login={me} node={node} claimed_at={claimed} expires_at={expires} -->\n\
         Claimed by the GAH loop of {me} on `{node}` until {expires}; the loop renews this \
         claim while it works. If it lapses, {after_lapse}.",
        claimed = stamp(claimed_at),
        expires = stamp(expires),
    )
}

fn same_login(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn is_issue_number(target: &str) -> bool {
    !target.is_empty() && target.bytes().all(|byte| byte.is_ascii_digit())
}

/// The login `gh` is signed in as: the identity this loop claims under.
fn own_login() -> Result<String> {
    let login = gh("read own login", &["api", "user", "--jq", ".login"])?;
    let login = login.trim();
    if login.is_empty() {
        anyhow::bail!("GitHub returned no login for the signed-in account");
    }
    Ok(login.to_string())
}

fn gh(operation: &str, args: &[&str]) -> Result<String> {
    gh_until(operation, args, None)
}

/// Run `gh`, killed at `deadline` when one is given, so a hung call cannot
/// outlast the authority it is meant to extend.
fn gh_until(operation: &str, args: &[&str], deadline: Option<Deadline>) -> Result<String> {
    let mut command = provider_command("gh");
    command.args(args);
    let launching = || format!("launching gh to {operation}");
    let out = match deadline {
        None => command.output().with_context(launching)?,
        Some(deadline) => {
            if deadline.reached() {
                anyhow::bail!("could not {operation}: the claim's lease has no time left");
            }
            let child = command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .with_context(launching)?;
            crate::worktree::wait_with_deadline_for(child, &format!("gh to {operation}"), deadline)?
        }
    };
    if !out.status.success() {
        anyhow::bail!(
            "could not {operation}: {}",
            crate::redact::redact(&String::from_utf8_lossy(&out.stderr)).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

struct GithubBoard<'a> {
    profile: &'a Profile,
    /// When set, every `gh` call is killed at this instant.
    deadline: Option<Deadline>,
}

impl<'a> GithubBoard<'a> {
    fn for_reading(profile: &'a Profile) -> Result<Self> {
        if !profile.provider.eq_ignore_ascii_case("github") {
            anyhow::bail!(
                "issue_claim.mode = \"github_assignee\" needs a GitHub profile, not '{}'",
                profile.provider
            );
        }
        Ok(Self {
            profile,
            deadline: None,
        })
    }

    fn for_writing(profile: &'a Profile) -> Result<Self> {
        crate::policy::enforce_policy(profile, "edit-issue")?;
        if !profile.publishing.allow_issue_comments {
            anyhow::bail!(
                "issue_claim.mode = \"github_assignee\" posts claim comments, \
                 but allow_issue_comments is false"
            );
        }
        Self::for_reading(profile)
    }

    fn gh(&self, operation: &str, args: &[&str]) -> Result<String> {
        gh_until(operation, args, self.deadline)
    }

    fn issue_endpoint(&self, issue: &str, suffix: &str) -> Result<String> {
        if !is_issue_number(issue) {
            anyhow::bail!("invalid issue number '{issue}': expected digits only");
        }
        Ok(format!(
            "repos/{}/issues/{issue}{suffix}",
            self.profile.repo
        ))
    }

    /// Every claim comment on the issue, whoever wrote it.
    fn claims(&self, issue: &str) -> Result<Vec<ClaimComment>> {
        let endpoint = self.issue_endpoint(issue, "/comments?per_page=100")?;
        let filter = format!(
            ".[] | select(.body | startswith(\"{CLAIM_MARKER}\")) \
             | {{id, login: (.user.login // \"\"), created_at, updated_at}}"
        );
        let lines = self.gh(
            "read claim comments",
            &[
                "api",
                "--method",
                "GET",
                "--paginate",
                &endpoint,
                "--jq",
                &filter,
            ],
        )?;
        lines
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let value: serde_json::Value =
                    serde_json::from_str(line).context("parsing a claim comment")?;
                let time = |field: &str| {
                    value[field]
                        .as_str()
                        .and_then(|at| OffsetDateTime::parse(at, &Rfc3339).ok())
                };
                match (
                    value["id"].as_u64(),
                    value["login"].as_str(),
                    time("created_at"),
                ) {
                    (Some(id), Some(login), Some(created_at)) => Ok(ClaimComment {
                        id,
                        login: login.to_string(),
                        created_at,
                        renewed_at: time("updated_at").map_or(created_at, |at| at.max(created_at)),
                    }),
                    _ => anyhow::bail!("claim comment is missing its id, author, or time"),
                }
            })
            .collect()
    }

    fn change_assignees(&self, issue: &str, method: &str, logins: &[String]) -> Result<String> {
        let endpoint = self.issue_endpoint(issue, "/assignees")?;
        let fields: Vec<String> = logins
            .iter()
            .map(|login| format!("assignees[]={login}"))
            .collect();
        let mut args = vec!["api", "--method", method, endpoint.as_str()];
        for field in &fields {
            args.extend(["-f", field.as_str()]);
        }
        args.extend(["--jq", ".assignees[].login"]);
        self.gh("change issue assignees", &args)
    }
}

impl IssueBoard for GithubBoard<'_> {
    fn view(&self, issue: &str) -> Result<IssueClaimView> {
        let endpoint = self.issue_endpoint(issue, "")?;
        let assignees = self.gh(
            "read issue assignees",
            &[
                "api",
                "--method",
                "GET",
                &endpoint,
                "--jq",
                ".assignees[].login",
            ],
        )?;
        Ok(IssueClaimView {
            assignees: assignees.lines().map(str::to_string).collect(),
            claims: self.claims(issue)?,
        })
    }

    fn assign(&self, issue: &str, login: &str) -> Result<()> {
        // GitHub answers success and changes nothing when the login may not
        // be assigned. Catch that here, before a claim comment is posted
        // that could never take effect.
        let assigned = self.change_assignees(issue, "POST", &[login.to_string()])?;
        if !assigned.lines().any(|line| same_login(line, login)) {
            anyhow::bail!("GitHub did not assign {login} to issue #{issue}; check its access");
        }
        Ok(())
    }

    fn unassign(&self, issue: &str, logins: &[String]) -> Result<()> {
        self.change_assignees(issue, "DELETE", logins).map(drop)
    }

    fn post_claim(&self, issue: &str, body: &str) -> Result<()> {
        crate::provider::post_issue_comment(self.profile, issue, body)
    }

    /// Editing the claim comment moves its `updated_at`, which is the
    /// renewal. An edit notifies nobody, unlike a fresh comment.
    fn renew_claim(&self, comment_id: u64, body: &str) -> Result<()> {
        crate::policy::enforce_policy(self.profile, "edit-issue")?;
        let endpoint = format!("repos/{}/issues/comments/{comment_id}", self.profile.repo);
        let body = format!("body={body}");
        self.gh(
            "renew claim comment",
            &[
                "api", "--method", "PATCH", &endpoint, "-f", &body, "--silent",
            ],
        )
        .map(drop)
    }

    fn has_open_pull_request(&self, issue: &str) -> Result<bool> {
        let aliases = crate::ledger::work_id_aliases(&format!("#{issue}"));
        Ok(crate::sync::fetch_active_mrs(self.profile)?
            .iter()
            .any(|mr| {
                mr.work_id
                    .as_deref()
                    .is_some_and(|id| aliases.iter().any(|alias| alias == id))
                    && !matches!(
                        crate::sync::classify(mr),
                        "MERGED" | "CLOSED_UNMERGED" | "STALE"
                    )
            }))
    }
}

#[cfg(test)]
#[path = "issue_claim/tests.rs"]
mod tests;
