use super::*;
use crate::dispatch::test_util::profile;
use crate::test_support::{ExecGuard, PathGuard};
use std::cell::RefCell;
use std::rc::Rc;

const MICHAEL: &str = "Ram-Rod6198";
const COLTON: &str = "Kh1ng";

fn policy(priority_logins: &[&str]) -> IssueClaimPolicy {
    IssueClaimPolicy {
        mode: IssueClaimMode::GithubAssignee,
        priority_logins: priority_logins.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    }
}

fn at(minutes: i64) -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minutes)
}

fn claim(id: u64, login: &str, minutes: i64) -> ClaimComment {
    ClaimComment {
        id,
        login: login.to_string(),
        created_at: at(minutes),
        renewed_at: at(minutes),
    }
}

/// Issue 7, the item most tests claim.
fn issue_7() -> ClaimTarget {
    ClaimTarget::Issue("7".into())
}

/// Pull request 12, the item of review and fix work.
fn pull_request_12() -> ClaimTarget {
    ClaimTarget::PullRequest("12".into())
}

fn won(expires_minutes: i64) -> ClaimOutcome {
    ClaimOutcome::Won {
        expires_at: at(expires_minutes),
    }
}

fn view(assignees: &[&str], claims: Vec<ClaimComment>) -> IssueClaimView {
    IssueClaimView {
        assignees: assignees.iter().map(|l| l.to_string()).collect(),
        claims,
    }
}

/// One issue shared by every loop in a test.
#[derive(Default)]
struct Issue {
    view: IssueClaimView,
    next_comment_id: u64,
    open_pull_request: bool,
    comments_fail: bool,
}

/// One loop's handle on the shared issue, acting under its own login.
struct LoopBoard {
    login: &'static str,
    issue: Rc<RefCell<Issue>>,
    now: OffsetDateTime,
}

impl IssueBoard for LoopBoard {
    fn view(&self, _issue: &str) -> Result<IssueClaimView> {
        Ok(self.issue.borrow().view.clone())
    }

    fn assign(&self, _issue: &str, login: &str) -> Result<()> {
        let assignees = &mut self.issue.borrow_mut().view.assignees;
        if !assignees.iter().any(|assigned| assigned == login) {
            assignees.push(login.to_string());
        }
        Ok(())
    }

    fn unassign(&self, _issue: &str, logins: &[String]) -> Result<()> {
        self.issue
            .borrow_mut()
            .view
            .assignees
            .retain(|assigned| !logins.contains(assigned));
        Ok(())
    }

    fn post_claim(&self, _issue: &str, _body: &str) -> Result<()> {
        let mut issue = self.issue.borrow_mut();
        if issue.comments_fail {
            anyhow::bail!("comment refused");
        }
        issue.next_comment_id += 1;
        let id = issue.next_comment_id;
        issue.view.claims.push(ClaimComment {
            id,
            login: self.login.to_string(),
            created_at: self.now,
            renewed_at: self.now,
        });
        Ok(())
    }

    fn renew_claim(&self, comment_id: u64, _body: &str) -> Result<()> {
        let mut issue = self.issue.borrow_mut();
        let comment = issue
            .view
            .claims
            .iter_mut()
            .find(|claim| claim.id == comment_id)
            .context("no such comment")?;
        comment.renewed_at = self.now;
        Ok(())
    }

    fn has_open_pull_request(&self, _issue: &str) -> Result<bool> {
        Ok(self.issue.borrow().open_pull_request)
    }
}

fn two_loops(issue: &Rc<RefCell<Issue>>, minutes: i64) -> (LoopBoard, LoopBoard) {
    let board = |login| LoopBoard {
        login,
        issue: Rc::clone(issue),
        now: at(minutes),
    };
    (board(MICHAEL), board(COLTON))
}

/// Colton stakes first, but Michael reads the issue before Colton's
/// assignment is visible, so both post a claim.
fn stake_both_at_once(
    issue: &Rc<RefCell<Issue>>,
    michael: &LoopBoard,
    colton: &LoopBoard,
    policy: &IssueClaimPolicy,
) {
    assert_eq!(
        stake(colton, policy, COLTON, &issue_7(), at(0)).unwrap(),
        None
    );
    issue.borrow_mut().view.assignees.clear();
    assert_eq!(
        stake(michael, policy, MICHAEL, &issue_7(), at(0)).unwrap(),
        None
    );
    issue.borrow_mut().view.assignees.push(COLTON.into());
}

fn assignees(issue: &Rc<RefCell<Issue>>) -> Vec<String> {
    issue.borrow().view.assignees.clone()
}

#[test]
fn the_earliest_claim_comment_wins_a_contested_claim() {
    let contested = view(
        &[MICHAEL, COLTON],
        vec![claim(11, COLTON, 0), claim(12, MICHAEL, 0)],
    );
    let policy = policy(&[]);

    assert_eq!(
        standing(&contested, MICHAEL, &policy, at(1)),
        Standing::Theirs(COLTON.into())
    );
    assert_eq!(
        standing(&contested, COLTON, &policy, at(1)),
        Standing::Mine { expires_at: at(60) }
    );
}

#[test]
fn a_priority_login_wins_over_an_earlier_claim_comment() {
    let contested = view(
        &[COLTON, MICHAEL],
        vec![claim(11, COLTON, 0), claim(12, MICHAEL, 0)],
    );
    let policy = policy(&["ram-rod6198"]);

    assert_eq!(
        standing(&contested, MICHAEL, &policy, at(1)),
        Standing::Mine { expires_at: at(60) }
    );
    assert_eq!(
        standing(&contested, COLTON, &policy, at(1)),
        Standing::Theirs(MICHAEL.into())
    );
}

#[test]
fn priority_does_not_take_an_issue_from_a_login_already_holding_it() {
    let held = view(&[COLTON], vec![claim(11, COLTON, 0)]);

    assert_eq!(
        standing(&held, MICHAEL, &policy(&[MICHAEL]), at(30)),
        Standing::Theirs(COLTON.into())
    );
}

#[test]
fn a_claim_holds_until_its_ttl_and_not_a_minute_longer() {
    let held = view(&[COLTON], vec![claim(11, COLTON, 0)]);
    let policy = policy(&[]);

    assert_eq!(
        standing(&held, MICHAEL, &policy, at(59)),
        Standing::Theirs(COLTON.into())
    );
    assert_eq!(
        standing(&held, MICHAEL, &policy, at(60)),
        Standing::Stale(vec![COLTON.into()])
    );
    // An unrenewed claim lapses in the holder's own view too.
    assert_eq!(
        standing(&held, COLTON, &policy, at(60)),
        Standing::Unclaimed
    );
}

#[test]
fn only_the_latest_claim_of_a_login_counts() {
    let reclaimed = view(&[COLTON], vec![claim(11, COLTON, 0), claim(40, COLTON, 90)]);

    assert_eq!(
        standing(&reclaimed, MICHAEL, &policy(&[]), at(100)),
        Standing::Theirs(COLTON.into())
    );
}

#[test]
fn a_hand_assigned_issue_is_held_without_any_claim_comment() {
    let by_hand = view(&[COLTON], vec![]);

    assert_eq!(
        standing(&by_hand, MICHAEL, &policy(&[MICHAEL]), at(100_000)),
        Standing::Theirs(COLTON.into())
    );
    // Assigned to this loop's own login, it is this loop's to claim.
    assert_eq!(
        standing(&by_hand, COLTON, &policy(&[]), at(0)),
        Standing::Unclaimed
    );
}

#[test]
fn a_claim_comment_from_a_login_that_is_not_assigned_holds_nothing() {
    let withdrawn = view(&[], vec![claim(11, COLTON, 0)]);

    assert_eq!(
        standing(&withdrawn, MICHAEL, &policy(&[]), at(1)),
        Standing::Unclaimed
    );
}

#[test]
fn two_loops_racing_for_one_issue_leave_exactly_one_holder() {
    let issue = Rc::new(RefCell::new(Issue::default()));
    let (michael, colton) = two_loops(&issue, 0);
    let policy = policy(&[]);

    stake_both_at_once(&issue, &michael, &colton, &policy);

    assert_eq!(
        settle(&michael, &policy, MICHAEL, &issue_7(), at(1)).unwrap(),
        ClaimOutcome::Lost(format!("yielded to {COLTON}"))
    );
    assert_eq!(
        settle(&colton, &policy, COLTON, &issue_7(), at(1)).unwrap(),
        won(60)
    );
    assert_eq!(assignees(&issue), [COLTON]);
}

#[test]
fn the_priority_login_wins_the_race_whoever_commented_first() {
    let issue = Rc::new(RefCell::new(Issue::default()));
    let (michael, colton) = two_loops(&issue, 0);
    let policy = policy(&[MICHAEL]);

    stake_both_at_once(&issue, &michael, &colton, &policy);

    assert_eq!(
        settle(&colton, &policy, COLTON, &issue_7(), at(1)).unwrap(),
        ClaimOutcome::Lost(format!("yielded to {MICHAEL}"))
    );
    assert_eq!(
        settle(&michael, &policy, MICHAEL, &issue_7(), at(1)).unwrap(),
        won(60)
    );
    assert_eq!(assignees(&issue), [MICHAEL]);
}

#[test]
fn a_held_issue_is_left_untouched() {
    let issue = Rc::new(RefCell::new(Issue::default()));
    let (michael, colton) = two_loops(&issue, 0);
    let policy = policy(&[MICHAEL]);
    assert_eq!(
        stake(&colton, &policy, COLTON, &issue_7(), at(0)).unwrap(),
        None
    );

    assert_eq!(
        stake(&michael, &policy, MICHAEL, &issue_7(), at(5)).unwrap(),
        Some(ClaimOutcome::Lost(format!("held by {COLTON}")))
    );
    assert_eq!(assignees(&issue), [COLTON]);
    assert_eq!(issue.borrow().view.claims.len(), 1);
}

#[test]
fn an_expired_claim_is_taken_over_by_removing_the_stale_assignee() {
    let issue = Rc::new(RefCell::new(Issue::default()));
    let (michael, colton) = two_loops(&issue, 61);
    let policy = policy(&[]);
    issue.borrow_mut().view = view(&[COLTON], vec![claim(1, COLTON, 0)]);
    issue.borrow_mut().next_comment_id = 1;

    assert_eq!(
        stake(&michael, &policy, MICHAEL, &issue_7(), at(61)).unwrap(),
        None
    );
    assert_eq!(assignees(&issue), [MICHAEL]);
    assert_eq!(
        settle(&michael, &policy, MICHAEL, &issue_7(), at(62)).unwrap(),
        won(121)
    );
    // The previous holder now sees a live claim that is not its own.
    assert_eq!(
        stake(&colton, &policy, COLTON, &issue_7(), at(62)).unwrap(),
        Some(ClaimOutcome::Lost(format!("held by {MICHAEL}")))
    );
}

#[test]
fn an_open_pull_request_keeps_an_expired_claim() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[COLTON], vec![claim(1, COLTON, 0)]),
        open_pull_request: true,
        ..Default::default()
    }));
    let (michael, _) = two_loops(&issue, 600);

    let outcome = stake(&michael, &policy(&[MICHAEL]), MICHAEL, &issue_7(), at(600)).unwrap();

    assert!(
        matches!(outcome, Some(ClaimOutcome::Lost(reason)) if reason.contains("open pull request"))
    );
    assert_eq!(assignees(&issue), [COLTON]);
}

#[test]
fn a_live_claim_of_this_loop_is_reused_without_a_new_comment() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[MICHAEL], vec![claim(1, MICHAEL, 0)]),
        ..Default::default()
    }));
    let (michael, _) = two_loops(&issue, 30);

    // The reused claim keeps its own expiry, a TTL after it was posted, not
    // a fresh TTL from now.
    assert_eq!(
        stake(&michael, &policy(&[]), MICHAEL, &issue_7(), at(30)).unwrap(),
        Some(won(60))
    );
    assert_eq!(issue.borrow().view.claims.len(), 1);
}

#[test]
fn a_renewed_claim_outlives_its_ttl_and_lapses_a_ttl_after_its_last_renewal() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[COLTON], vec![claim(1, COLTON, 0)]),
        ..Default::default()
    }));
    let policy = policy(&[]);
    let (_, colton) = two_loops(&issue, 40);

    assert_eq!(
        renew(&colton, &policy, COLTON, &issue_7(), at(40)).unwrap(),
        None
    );

    let seen = issue.borrow().view.clone();
    assert_eq!(
        standing(&seen, MICHAEL, &policy, at(99)),
        Standing::Theirs(COLTON.into())
    );
    assert_eq!(
        standing(&seen, MICHAEL, &policy, at(100)),
        Standing::Stale(vec![COLTON.into()])
    );
    // Renewal keeps the claim's place in a contest: its comment id is unchanged.
    assert_eq!(seen.claims[0].id, 1);
}

#[test]
fn a_lease_taken_over_after_it_lapsed_is_reported_lost_and_not_renewed() {
    let issue = Rc::new(RefCell::new(Issue::default()));
    let policy = policy(&[]);
    let (michael, colton) = two_loops(&issue, 0);
    assert_eq!(
        stake(&colton, &policy, COLTON, &issue_7(), at(0)).unwrap(),
        None
    );

    // Colton's renewals stopped; Michael takes the lapsed claim over.
    let (michael_later, colton_later) = two_loops(&issue, 61);
    drop((michael, colton));
    assert_eq!(
        stake(&michael_later, &policy, MICHAEL, &issue_7(), at(61)).unwrap(),
        None
    );

    let lost = renew(&colton_later, &policy, COLTON, &issue_7(), at(62)).unwrap();

    assert_eq!(lost, Some(format!("lease taken over by {MICHAEL}")));
    assert_eq!(assignees(&issue), [MICHAEL]);
    let colton_claim = issue.borrow().view.claims[0].clone();
    assert_eq!(colton_claim.renewed_at, at(0));
}

#[test]
fn a_lost_lease_stops_the_dispatch_with_a_claim_lost_error() {
    let lease = IssueLease {
        target: issue_7(),
        state: Arc::new(Mutex::new(LeaseState {
            lost: Some(LeaseLoss::Claim(format!("lease taken over by {MICHAEL}"))),
            deadline: Instant::now() + Duration::from_secs(3600),
        })),
        stop: None,
        renewer: None,
    };

    let error = lease.ensure_held().unwrap_err();

    assert!(issue_claim_lost(&error).is_some());
    assert!(!crate::dispatch::terminal::should_notify_dispatch_failure(
        &error
    ));
}

#[test]
fn a_claim_whose_comment_cannot_be_posted_leaves_no_assignee_behind() {
    let issue = Rc::new(RefCell::new(Issue {
        comments_fail: true,
        ..Default::default()
    }));
    let (michael, _) = two_loops(&issue, 0);

    assert!(stake(&michael, &policy(&[]), MICHAEL, &issue_7(), at(0)).is_err());
    assert!(assignees(&issue).is_empty());
}

#[test]
fn only_a_github_issue_target_is_claimed_on_the_provider() {
    let tmp = tempfile::tempdir().unwrap();
    let mut prof = profile(tmp.path());
    assert_eq!(provider_claimed_issue(&prof, "7"), None, "local claims");

    prof.publishing.issue_claim.mode = IssueClaimMode::GithubAssignee;
    // `gah dispatch` takes `#7` for the same issue the loop dispatches as `7`.
    for target in ["7", "#7", " #7 "] {
        assert_eq!(
            provider_claimed_issue(&prof, target).as_deref(),
            Some("7"),
            "{target:?}"
        );
    }
    // Work with no GitHub issue skips the provider claim; it is no error.
    for target in [
        "docs/tickets/TICKET-007-example.md",
        "",
        "#",
        "fix the tests",
    ] {
        assert_eq!(provider_claimed_issue(&prof, target), None, "{target:?}");
    }
}

#[test]
fn a_person_assigned_next_to_an_expired_claim_keeps_every_loop_out() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[COLTON, "a-person"], vec![claim(1, COLTON, 0)]),
        next_comment_id: 1,
        ..Default::default()
    }));
    let (michael, _) = two_loops(&issue, 600);

    let outcome = stake(&michael, &policy(&[MICHAEL]), MICHAEL, &issue_7(), at(600)).unwrap();

    assert_eq!(outcome, Some(ClaimOutcome::Lost("held by a-person".into())));
    // Nothing is removed, not even the other loop's expired assignee.
    assert_eq!(assignees(&issue), [COLTON, "a-person"]);
    assert_eq!(issue.borrow().view.claims.len(), 1);
}

#[test]
fn a_person_assigned_while_a_loop_works_ends_its_lease_and_stays_assigned() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[MICHAEL, "a-person"], vec![claim(1, MICHAEL, 0)]),
        ..Default::default()
    }));
    let (michael, _) = two_loops(&issue, 20);

    let lost = renew(&michael, &policy(&[MICHAEL]), MICHAEL, &issue_7(), at(20)).unwrap();

    assert_eq!(lost, Some("lease taken over by a-person".into()));
    assert_eq!(assignees(&issue), [MICHAEL, "a-person"]);
}

#[test]
fn an_old_claim_renewed_within_its_ttl_is_not_reclaimable() {
    let mut renewed = claim(1, COLTON, 0);
    renewed.renewed_at = at(170);
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[COLTON], vec![renewed]),
        next_comment_id: 1,
        ..Default::default()
    }));
    let (michael, _) = two_loops(&issue, 200);

    // Posted 200 minutes ago, more than three TTLs, but renewed 30 minutes ago.
    let outcome = stake(&michael, &policy(&[MICHAEL]), MICHAEL, &issue_7(), at(200)).unwrap();

    assert_eq!(
        outcome,
        Some(ClaimOutcome::Lost(format!("held by {COLTON}")))
    );
    assert_eq!(assignees(&issue), [COLTON]);
}

#[test]
fn a_contender_seen_only_after_the_verify_pause_wins_the_claim() {
    // Issues and pull requests share the protocol, pause included.
    for target in [issue_7(), pull_request_12()] {
        let issue = Rc::new(RefCell::new(Issue::default()));
        let (michael, colton) = two_loops(&issue, 0);
        let policy = policy(&[COLTON]);

        // Colton read the item before Michael's assignment landed, and his
        // own claim lands while Michael waits out the pause.
        let outcome = run_claim(&michael, &policy, MICHAEL, &target, at(0), || {
            let seen_by_michael = std::mem::take(&mut issue.borrow_mut().view.assignees);
            assert_eq!(
                stake(&colton, &policy, COLTON, &target, at(0)).unwrap(),
                None
            );
            issue
                .borrow_mut()
                .view
                .assignees
                .splice(0..0, seen_by_michael);
            at(1)
        })
        .unwrap();

        // Deciding before the pause, Michael would have kept the item too.
        assert_eq!(
            outcome,
            ClaimOutcome::Lost(format!("yielded to {COLTON}")),
            "{target}"
        );
        assert_eq!(
            settle(&colton, &policy, COLTON, &target, at(1)).unwrap(),
            won(60)
        );
        assert_eq!(assignees(&issue), [COLTON]);
    }
}

#[test]
fn an_expired_pull_request_claim_is_free_although_the_pull_request_is_open() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[COLTON], vec![claim(1, COLTON, 0)]),
        next_comment_id: 1,
        open_pull_request: true,
        ..Default::default()
    }));
    let policy = policy(&[]);

    // Within its TTL the claim holds the pull request like any other.
    let (michael, _) = two_loops(&issue, 59);
    assert_eq!(
        stake(&michael, &policy, MICHAEL, &pull_request_12(), at(59)).unwrap(),
        Some(ClaimOutcome::Lost(format!("held by {COLTON}")))
    );
    // Lapsed, it holds nothing: the open pull request is the item itself,
    // not work that carries the claim on (compare the issue rule in
    // `an_open_pull_request_keeps_an_expired_claim`).
    let (michael, _) = two_loops(&issue, 60);
    assert_eq!(
        stake(&michael, &policy, MICHAEL, &pull_request_12(), at(60)).unwrap(),
        None
    );
    assert_eq!(assignees(&issue), [MICHAEL]);
    assert_eq!(
        settle(&michael, &policy, MICHAEL, &pull_request_12(), at(61)).unwrap(),
        won(120)
    );
}

#[test]
fn a_person_assigned_to_a_pull_request_keeps_every_loop_out() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[COLTON, "a-person"], vec![claim(1, COLTON, 0)]),
        next_comment_id: 1,
        ..Default::default()
    }));
    let (michael, _) = two_loops(&issue, 600);

    let outcome = stake(
        &michael,
        &policy(&[MICHAEL]),
        MICHAEL,
        &pull_request_12(),
        at(600),
    );

    assert_eq!(
        outcome.unwrap(),
        Some(ClaimOutcome::Lost("held by a-person".into()))
    );
    // Nothing is removed or added, not even the other loop's expired claim.
    assert_eq!(assignees(&issue), [COLTON, "a-person"]);
    assert_eq!(issue.borrow().view.claims.len(), 1);
}

#[test]
fn renewals_are_scheduled_from_the_confirmed_expiry() {
    let minutes = |n: u64| Duration::from_secs(n * 60);
    let ttl = minutes(60);

    // A fresh claim renews once a third of its TTL has passed.
    assert_eq!(next_renewal_wait(minutes(60), ttl, false), minutes(20));
    // An inherited claim with one minute left renews at once, not 20 minutes on.
    assert_eq!(next_renewal_wait(minutes(1), ttl, false), Duration::ZERO);
    // After a failure, retry while the claim is still live.
    assert_eq!(next_renewal_wait(minutes(10), ttl, true), minutes(5));
    let last_moment = Duration::from_millis(400);
    assert_eq!(next_renewal_wait(last_moment, ttl, true), last_moment);
}

#[test]
fn a_renewal_confirmed_after_the_deadline_loses_the_lease_for_good() {
    let start = Instant::now();
    let deadline = start + Duration::from_millis(150);
    let lease = IssueLease {
        target: issue_7(),
        state: Arc::new(Mutex::new(LeaseState {
            lost: None,
            deadline,
        })),
        stop: None,
        renewer: None,
    };
    let extended = start + Duration::from_secs(3600);

    // The renewal call returned success, but only seen 10 ms after the old
    // deadline, and nobody called `ensure_held` in between.
    let held = confirm_renewal(&lease.state, extended, deadline + Duration::from_millis(10));

    assert!(!held);
    assert_eq!(lock(&lease.state).deadline, deadline, "not extended");
    for _ in 0..2 {
        let error = lease.ensure_held().unwrap_err();
        assert!(issue_claim_lost(&error).is_some());
        assert!(error.to_string().contains("expired"), "{error:#}");
    }
    // A later on-time success cannot revive it.
    assert!(!confirm_renewal(&lease.state, extended, start));
    assert!(lease.ensure_held().is_err());
}

#[test]
fn a_renewal_confirmed_before_the_deadline_extends_the_lease() {
    let start = Instant::now();
    let state = Mutex::new(LeaseState {
        lost: None,
        deadline: start + Duration::from_millis(150),
    });
    let extended = start + Duration::from_secs(3600);

    assert!(confirm_renewal(
        &state,
        extended,
        start + Duration::from_millis(140)
    ));
    assert_eq!(lock(&state).deadline, extended);
}

#[test]
fn the_lease_expires_at_its_deadline_without_the_renewer() {
    // No renewer runs and nothing marked the lease lost: only the deadline.
    let lease = IssueLease {
        target: issue_7(),
        state: Arc::new(Mutex::new(LeaseState {
            lost: None,
            deadline: Instant::now() + Duration::from_millis(50),
        })),
        stop: None,
        renewer: None,
    };
    assert!(lease.ensure_held().is_ok());

    std::thread::sleep(Duration::from_millis(60));

    let error = lease.ensure_held().unwrap_err();
    assert!(issue_claim_lost(&error).is_some());
    assert!(error.to_string().contains("expired"), "{error:#}");
}

/// A `gh` on `PATH` running `body` for every call, with `$marker` naming a
/// file it may append to. Returns the marker path.
fn install_renewal_gh(bin_dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(bin_dir).unwrap();
    let marker = bin_dir.join("calls");
    let gh_path = bin_dir.join("gh");
    std::fs::write(
        &gh_path,
        format!("#!/bin/sh\nmarker='{}'\n{body}\n", marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    marker
}

fn wait_for(condition: impl Fn() -> bool, limit: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    condition()
}

#[test]
fn a_hung_renewal_cannot_keep_a_lease_past_its_confirmed_expiry() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    // Every provider call hangs far past the claim's expiry.
    let marker = install_renewal_gh(&bin_dir, "echo call >> \"$marker\"\nexec sleep 30");
    let _guard = PathGuard::set(&bin_dir);
    let mut prof = profile(tmp.path());
    prof.publishing.issue_claim = policy(&[]);

    // An inherited claim with 3 seconds of its hour left.
    let deadline = Instant::now() + Duration::from_secs(3);
    let lease = IssueLease::start(
        &prof,
        &issue_7(),
        "me".into(),
        deadline,
        claim_ttl(&prof.publishing.issue_claim),
    );

    assert!(
        wait_for(|| marker.exists(), Duration::from_millis(2500)),
        "the first renewal must start at once"
    );
    assert!(lease.ensure_held().is_ok());
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(20),
    );

    // The renewal call is still blocked, yet the lease is over.
    let error = lease.ensure_held().unwrap_err();
    assert!(error.to_string().contains("expired"), "{error:#}");
    // The hung call is cut off at the expiry, so stopping the lease does not
    // wait for it to finish on its own.
    let stopping = Instant::now();
    drop(lease);
    assert!(stopping.elapsed() < Duration::from_secs(10));
}

#[test]
fn an_aging_claim_is_renewed_at_once_and_keeps_its_authority() {
    let issue = Rc::new(RefCell::new(Issue {
        view: view(&[COLTON], vec![claim(21, COLTON, 0)]),
        ..Default::default()
    }));
    let policy = policy(&[]);
    let (_, colton) = two_loops(&issue, 59);
    let ttl = claim_ttl(&policy);
    // Put the old expiry in the past and confirm at a simulated earlier time.
    // No subprocess or scheduler must finish inside a three-second lease.
    let deadline = Instant::now() - Duration::from_secs(1);
    let began = deadline - Duration::from_secs(3);
    let lease = IssueLease {
        target: issue_7(),
        state: Arc::new(Mutex::new(LeaseState {
            lost: None,
            deadline,
        })),
        stop: None,
        renewer: None,
    };

    assert_eq!(
        next_renewal_wait(deadline - began, ttl, false),
        Duration::ZERO
    );
    assert_eq!(
        renew(&colton, &policy, COLTON, &issue_7(), at(59)).unwrap(),
        None
    );
    assert!(confirm_renewal(&lease.state, began + ttl, began));

    // Even past the old expiry, both the local lease and provider claim hold.
    assert!(lease.ensure_held().is_ok());
    assert_eq!(lock(&lease.state).deadline, began + ttl);
    assert_eq!(
        standing(&issue.borrow().view, MICHAEL, &policy, at(61)),
        Standing::Theirs(COLTON.into())
    );
    assert_eq!(issue.borrow().view.claims[0].id, 21);
}

/// Clock and confirmation channel carried from `start` into its renewer.
pub(super) struct RenewalProbe {
    pub(super) now: Arc<Mutex<Instant>>,
    pub(super) confirmed: mpsc::Sender<()>,
}

thread_local! {
    pub(super) static RENEWAL_PROBE: RefCell<Option<RenewalProbe>> = const { RefCell::new(None) };
}

#[test]
fn start_renews_an_aging_claim_in_the_background() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let claimed_at = (OffsetDateTime::now_utc() - time::Duration::minutes(59))
        .format(&Rfc3339)
        .unwrap();
    let calls = install_renewal_gh(
        &bin,
        &format!(
            r#"case "$*" in
  *'PATCH'*) echo renewed >> "$marker" ;;
  *'comments?'*) echo '{{"id":21,"login":"{COLTON}","created_at":"{claimed_at}"}}' ;;
  *) echo '{COLTON}' ;;
esac"#
        ),
    );
    let _path = PathGuard::set(&bin);
    let mut prof = profile(tmp.path());
    prof.publishing.issue_claim = policy(&[]);
    let began = Instant::now();
    // Give provider subprocesses a watchdog, while the injected clock makes
    // renewal and confirmation independent of their scheduling latency.
    let deadline = began + Duration::from_secs(120);
    let clock = Arc::new(Mutex::new(began));
    let (confirmed, receive) = mpsc::channel();
    RENEWAL_PROBE.with(|probe| {
        *probe.borrow_mut() = Some(RenewalProbe {
            now: Arc::clone(&clock),
            confirmed,
        })
    });
    let ttl = claim_ttl(&prof.publishing.issue_claim);
    let lease = IssueLease::start(&prof, &issue_7(), COLTON.into(), deadline, ttl);
    receive
        .recv_timeout(Duration::from_secs(120))
        .expect("start's background renewer did not confirm the aging claim at once");
    assert_eq!(std::fs::read_to_string(calls).unwrap(), "renewed\n");
    assert_eq!(lock(&lease.state).deadline, began + ttl);
    assert!(lease.ensure_held().is_ok());
    // Advance beyond the inherited expiry; confirmed authority remains.
    *clock.lock().unwrap() = deadline + Duration::from_secs(1);
    assert!(*clock.lock().unwrap() < lock(&lease.state).deadline);
    assert!(lock(&lease.state).lost.is_none());
    drop(lease);
}

/// A `gh` that answers the reads intake makes: the signed-in login, the
/// open-issue list, each issue's claim comments, and its (absent) sub-issues.
fn install_fake_gh(
    bin_dir: &std::path::Path,
    issues: &serde_json::Value,
    claims: &[(u64, String)],
) {
    let mut script = format!(
        "#!/bin/sh\ncase \"$*\" in\n  'api user '*) echo {MICHAEL} ;;\n  *'issues?state=open'*) cat <<'JSON'\n{issues}\nJSON\n  ;;\n"
    );
    for (number, lines) in claims {
        script.push_str(&format!(
            "  *'issues/{number}/comments'*) cat <<'JSON'\n{lines}\nJSON\n  ;;\n"
        ));
    }
    script.push_str(
        "  *'/comments'*) ;;\n  *'/sub_issues'*) echo '[]' ;;\n  *) echo \"unexpected gh call: $*\" >&2; exit 2 ;;\nesac\n",
    );
    let gh_path = bin_dir.join("gh");
    std::fs::write(&gh_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn intake_skips_issues_another_login_holds() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let issue = |number: u64, assignees: &[&str]| {
        serde_json::json!({
            "number": number,
            "title": format!("Issue {number}"),
            "body": "",
            "labels": [],
            "user": {"login": "owner", "type": "User"},
            "state": "open",
            "assignees": assignees.iter().map(|login| serde_json::json!({"login": login})).collect::<Vec<_>>(),
        })
    };
    let claim_line = |id: u64, age: time::Duration| {
        serde_json::json!({
            "id": id,
            "login": COLTON,
            "created_at": (OffsetDateTime::now_utc() - age).format(&Rfc3339).unwrap(),
        })
        .to_string()
    };
    install_fake_gh(
        &bin_dir,
        &serde_json::json!([
            issue(1, &[]),
            issue(2, &[COLTON]),
            issue(3, &[COLTON]),
            issue(4, &[COLTON]),
            issue(5, &[MICHAEL]),
        ]),
        &[
            (2, claim_line(21, time::Duration::minutes(5))),
            (4, claim_line(41, time::Duration::minutes(61))),
        ],
    );
    let _guard = PathGuard::set(&bin_dir);
    let mut prof = profile(tmp.path());
    // No label gate: an unlabelled issue is eligible in this mode.
    prof.publishing.issue_claim = policy(&[MICHAEL]);

    let scan = crate::dispatch::claims::scan_available_tickets_with_dependencies(
        &prof,
        &[],
        &Default::default(),
        &[],
    );

    assert_eq!(scan.provider_error, None);
    let held: Vec<(&str, bool)> = scan
        .available_tickets
        .iter()
        .map(|ticket| (ticket.ticket_path.as_str(), ticket.has_active_claim))
        .collect();
    assert_eq!(
        held,
        [
            ("1", false), // unassigned
            ("2", true),  // live claim by another login
            ("3", true),  // assigned by hand to another login
            ("4", false), // the other login's claim has expired
            ("5", false), // assigned to this loop's own login
        ]
    );
}

#[test]
fn intake_fails_closed_when_the_signed_in_login_is_unknown() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let gh_path = bin_dir.join("gh");
    std::fs::write(
        &gh_path,
        "#!/bin/sh\ncase \"$*\" in\n  'api user '*) echo 'not signed in' >&2; exit 1 ;;\n  *) echo '[]' ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);
    let mut prof = profile(tmp.path());
    prof.publishing.issue_claim = policy(&[]);

    let scan = crate::dispatch::claims::scan_available_tickets_with_dependencies(
        &prof,
        &[],
        &Default::default(),
        &[],
    );

    assert!(scan.available_tickets.is_empty());
    assert!(scan.provider_error.unwrap().contains("read own login"));
}

fn ledger_entry(prof: &Profile) -> LedgerEntry {
    LedgerEntry::new("test", prof, "auto", "fix", "7", None, None)
}

#[test]
fn a_dispatch_that_loses_its_claim_ends_as_a_skip_not_a_failure() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    // Issue 7 and pull request 12 are assigned to someone else by hand: no
    // claim comments exist.
    let gh_path = bin_dir.join("gh");
    std::fs::write(
        &gh_path,
        "#!/bin/sh\ncase \"$*\" in\n  'api user '*) echo me ;;\n  *'/comments'*) ;;\n  *'issues/7 '*|*'issues/12 '*) echo someone-else ;;\n  *) exit 2 ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _guard = PathGuard::set(&bin_dir);
    let mut prof = profile(tmp.path());
    prof.publishing.issue_claim = policy(&[]);

    // GitHub answers for pull request 12 on the issue endpoints as well.
    for (target, lost) in [
        (issue_7(), "issue #7: held by someone-else"),
        (pull_request_12(), "pull request #12: held by someone-else"),
    ] {
        let mut ledger = ledger_entry(&prof);

        let error = claim_or_lose(&prof, &target, &mut ledger).unwrap_err();

        assert_eq!(
            issue_claim_lost(&error).map(ToString::to_string).as_deref(),
            Some(lost)
        );
        assert!(crate::ledger::gates::launched_no_backend(&ledger));
        assert!(!crate::dispatch::terminal::should_notify_dispatch_failure(
            &error
        ));
    }
}

/// A `gh` that knows one pull request, 12, on branch `gah/fix-12`.
fn install_pull_request_gh(bin_dir: &std::path::Path) {
    let gh_path = bin_dir.join("gh");
    std::fs::write(
        &gh_path,
        "#!/bin/sh
case \"$*\" in
  *'repos/owner/repo/pulls '*'head=owner:gah/fix-12'*) echo '[{\"number\":12}]' ;;
  *) echo \"unexpected gh call: $*\" >&2; exit 2 ;;
esac
",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn review_and_fix_work_claims_its_pull_request_and_fails_closed_without_one() {
    let _exec_guard = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    install_pull_request_gh(&bin_dir);
    let _guard = PathGuard::set(&bin_dir);
    let mut prof = profile(tmp.path());
    let claims = |slot: Result<IssueClaimSlot>| slot.unwrap().target;

    // Local claims: nothing to claim, and no provider call to find it.
    assert_eq!(claims(IssueClaimSlot::for_review(&prof, None)), None);
    assert_eq!(
        claims(IssueClaimSlot::for_improve(&prof, "gah/x", Some("gah/x"))),
        None
    );

    prof.publishing.issue_claim = policy(&[]);
    assert_eq!(
        claims(IssueClaimSlot::for_review(&prof, Some("12"))),
        Some(pull_request_12())
    );
    // A fix of an existing branch claims the branch's pull request, not the
    // issue its target names.
    assert_eq!(
        claims(IssueClaimSlot::for_improve(&prof, "7", Some("gah/fix-12"))),
        Some(pull_request_12())
    );
    assert_eq!(
        claims(IssueClaimSlot::for_improve(&prof, "7", None)),
        Some(issue_7())
    );
    // A pull request that cannot be found or named is not claimed by
    // running unclaimed: the dispatch fails before any work.
    assert!(IssueClaimSlot::for_review(&prof, None).is_err());
    assert!(IssueClaimSlot::for_review(&prof, Some("https://github.com/o/r/pull/12")).is_err());
    assert!(IssueClaimSlot::for_improve(&prof, "7", Some("gah/unknown")).is_err());
}

#[test]
fn a_pull_request_claim_comment_does_not_promise_an_open_pull_request_hold() {
    let policy = policy(&[]);
    let issue = claim_comment(MICHAEL, &policy, &issue_7(), at(0), at(0));
    let pull_request = claim_comment(MICHAEL, &policy, &pull_request_12(), at(0), at(0));

    assert!(issue.contains("take this issue over, unless an open pull request references it"));
    assert!(pull_request.contains("another loop may take this pull request over."));
    assert!(pull_request.starts_with(CLAIM_MARKER));
}

#[test]
fn a_lost_claim_does_not_count_as_an_attempt_on_the_issue() {
    let tmp = tempfile::tempdir().unwrap();
    let ticket_dir = tmp.path().join("docs/tickets");
    std::fs::create_dir_all(&ticket_dir).unwrap();
    std::fs::write(
        ticket_dir.join("TICKET-302-test.md"),
        "# TICKET-302: Test lost claim\nGoal: remain dispatchable\n",
    )
    .unwrap();
    let mut prof = profile(tmp.path());
    prof.provider = String::new();
    let mut lost = ledger_entry(&prof);
    lost.work_id = Some("TICKET-302".into());
    lost.validation_result = Some(crate::ledger::gates::CLAIM_LOST.into());
    lost.failure_class = Some("harness_error".into());
    let index = crate::ledger::index_entries_by_work_id(&[lost]);

    let scan =
        crate::dispatch::claims::scan_available_tickets_with_dependencies(&prof, &[], &index, &[]);

    assert_eq!(scan.available_tickets.len(), 1);
    assert_eq!(scan.available_tickets[0].prior_attempt_count, 0);
    assert_eq!(scan.available_tickets[0].last_failure_class, None);
}

#[test]
fn claim_writes_refused_when_repo_policy_disallows_issue_writes() {
    let _exec = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let calls = install_renewal_gh(&bin, "echo called >> \"$marker\"; exit 1");
    let _path = PathGuard::set(&bin);
    let path = tmp.path().join("policy.toml");
    let read_only = include_str!("../../../tests/fixtures/policy-read-only.toml");
    for text in [
        read_only.to_string(),
        read_only
            .replace("read_only", "draft_pr_allowed")
            .replace("allow_issue_write = true", "allow_issue_write = false"),
    ] {
        std::fs::write(&path, text).unwrap();
        let mut prof = profile(tmp.path());
        prof.publishing.issue_claim = policy(&[]);
        prof.policy_path = Some(path.display().to_string());
        for target in [issue_7(), pull_request_12()] {
            let error = claim_or_lose(&prof, &target, &mut ledger_entry(&prof)).unwrap_err();
            assert!(crate::policy::policy_blocked(&error).is_some(), "{error:#}");
        }
        let board = GithubBoard::for_reading(&prof).unwrap();
        let error = board.renew_claim(1, "renewed").unwrap_err();
        assert!(crate::policy::policy_blocked(&error).is_some(), "{error:#}");
        assert!(
            !calls.exists(),
            "a policy refusal must not call the provider"
        );
    }
}

#[test]
fn policy_blocked_renewal_stops_and_preserves_the_policy_refusal() {
    let _exec = ExecGuard::new();
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
    let calls = install_renewal_gh(
        &bin,
        &format!(
            r#"
case "$*" in
  *PATCH*) echo write >> "$marker"; exit 1 ;;
  *comments*) echo '{{"id":1,"login":"me","created_at":"{now}","updated_at":"{now}"}}' ;;
  *) echo me ;;
esac
"#
        ),
    );
    let _path = PathGuard::set(&bin);
    let policy_file = tmp.path().join("policy.toml");
    let read_only = include_str!("../../../tests/fixtures/policy-read-only.toml");
    std::fs::write(
        &policy_file,
        read_only.replace("read_only", "draft_pr_allowed"),
    )
    .unwrap();
    let mut prof = profile(tmp.path());
    prof.publishing.issue_claim = policy(&[]);
    prof.policy_path = Some(policy_file.display().to_string());
    let board = GithubBoard::for_writing(&prof).unwrap();
    // An existing claim must recheck the file before its next renewal.
    std::fs::write(&policy_file, read_only).unwrap();
    assert!(crate::policy::policy_blocked(&board.renew_claim(1, "renewed").unwrap_err()).is_some());
    let lease = IssueLease::start(
        &prof,
        &issue_7(),
        "me".into(),
        Instant::now() + Duration::from_secs(30),
        claim_ttl(&prof.publishing.issue_claim),
    );
    assert!(wait_for(
        || lease.ensure_held().is_err(),
        Duration::from_secs(2)
    ));
    let error = lease.ensure_held().unwrap_err();
    assert!(crate::policy::policy_blocked(&error).is_some(), "{error:#}");
    assert!(issue_claim_lost(&error).is_none());
    drop(lease);
    assert!(!calls.exists(), "a refused renewal must never write");
}
