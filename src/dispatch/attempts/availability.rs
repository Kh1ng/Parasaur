use super::*;

/// Current instant, but with the *local* UTC offset attached rather than
/// always `+00:00`. `quota_parser::parse` treats a no-timezone time-of-day
/// string in backend output (e.g. Codex's "resets 9:01 PM") as being in
/// `now`'s offset, since that's the only clock available to a backend CLI
/// printing to its own terminal -- it means local wall-clock time, not
/// UTC. Passing `crate::clock::now_utc()` silently mis-resolved every
/// such reset by exactly the host's UTC offset (observed live: a ~3am
/// local reset displaying as "~14h remaining" on a UTC-5 host). Falls back
/// to UTC only if the local offset genuinely can't be determined.
fn now_with_local_offset() -> OffsetDateTime {
    let local_offset_seconds = chrono::Local::now().offset().local_minus_utc();
    let offset =
        time::UtcOffset::from_whole_seconds(local_offset_seconds).unwrap_or(time::UtcOffset::UTC);
    crate::clock::now_utc().to_offset(offset)
}

pub(in crate::dispatch) fn mark_backend_unavailable_from_output_for_identity(
    identity: &crate::execution_identity::ExecutionIdentity,
    log_text: &str,
    log_path: &str,
) -> Result<Option<crate::quota_parser::ParsedFailure>> {
    mark_backend_unavailable_from_output_for_identity_at(
        &crate::availability::resolve_state_path(),
        identity,
        log_text,
        log_path,
    )
}

pub(super) fn mark_backend_unavailable_from_output_for_identity_at(
    state_path: &Path,
    identity: &crate::execution_identity::ExecutionIdentity,
    log_text: &str,
    log_path: &str,
) -> Result<Option<crate::quota_parser::ParsedFailure>> {
    mark_failure(state_path, identity, log_text, log_path, None)
}

pub(super) fn mark_and_notify(
    cfg: &GahConfig,
    profile: &Profile,
    identity: &crate::execution_identity::ExecutionIdentity,
    log_text: &str,
    log_path: &str,
) -> Result<Option<crate::quota_parser::ParsedFailure>> {
    mark_failure(
        &crate::availability::resolve_state_path(),
        identity,
        log_text,
        log_path,
        Some((cfg, profile)),
    )
}

/// Failed reviews also mark exhaustion on their last allowed attempt.
pub(in crate::dispatch) fn record_review_outcome(
    cfg: &GahConfig,
    profile: &Profile,
    identity: &crate::execution_identity::ExecutionIdentity,
    run: &runner::ReviewRunResult,
) -> Result<()> {
    if matches!(run.outcome, runner::ReviewProcessOutcome::Success) {
        return Ok(());
    }
    let text = format!("{}\n{}", run.stdout, run.stderr);
    if crate::quota_parser::parse(&identity.logical_backend, &text, now_with_local_offset())
        .is_some_and(|failure| failure.kind == crate::quota_parser::FailureKind::QuotaExhausted)
    {
        mark_and_notify(cfg, profile, identity, &text, "review output")?;
    }
    Ok(())
}

/// Observe terminal quota errors even on the last allowed attempt. A zero exit can
/// still carry a provider rejection, so only an error-free result clears the streak.
pub(super) fn record_run_outcome(
    cfg: &GahConfig,
    profile: &Profile,
    identity: &crate::execution_identity::ExecutionIdentity,
    run: &runner::RunResult,
) -> Result<()> {
    let Ok(output) = fs::read_to_string(&run.log_path) else {
        return Ok(());
    };
    let text = failure_text_with_internal_log(&output, run.internal_log_delta.as_deref());
    let now = now_with_local_offset();
    let failure = crate::quota_parser::parse(&identity.logical_backend, &text, now);
    if run.exit_code != 0
        && failure
            .as_ref()
            .is_some_and(|failure| failure.kind == crate::quota_parser::FailureKind::QuotaExhausted)
    {
        mark_and_notify(cfg, profile, identity, &text, &run.log_path)?;
    } else if run.exit_code == 0 && failure.is_none() {
        crate::availability::clear_quota_failure(
            &crate::availability::resolve_state_path(),
            identity,
            now,
        )?;
    }
    Ok(())
}

fn mark_failure(
    state_path: &Path,
    identity: &crate::execution_identity::ExecutionIdentity,
    log_text: &str,
    log_path: &str,
    notify: Option<(&GahConfig, &Profile)>,
) -> Result<Option<crate::quota_parser::ParsedFailure>> {
    let backend = identity.logical_backend.as_str();
    let now = now_with_local_offset();
    // An idle watchdog kill is a backend outage signal, not an ordinary agent
    // failure. Keep this route out of the candidate set for a short bounded
    // cooldown so the next attempt can use another backend/model instead of
    // burning the same five-minute stall again. Both terminal-attribution
    // variants (stalled before changes / stalled during validation with
    // checkpointed changes) are idle-watchdog stalls and trigger the cooldown.
    if log_text.contains("GAH: killed after ")
        && log_text.contains("(stalled")
        && log_text.contains("not just slow).")
    {
        let cooldown = now + time::Duration::minutes(15);
        crate::availability::record_unavailable_for_identity(
            state_path,
            identity,
            crate::availability::Reason::BackendOutage,
            crate::availability::Source::BackendError,
            Some(cooldown),
            Some(format!(
                "backend idle watchdog stalled; cooldown=15m; log={log_path}"
            )),
            now,
        )?;
        return Ok(Some(crate::quota_parser::ParsedFailure {
            backend: backend.to_string(),
            kind: crate::quota_parser::FailureKind::BackendStalled,
            retryable: true,
            reset_at: Some(cooldown.format(&Rfc3339)?),
            retry_after_seconds: Some(15 * 60),
            confidence: crate::quota_parser::Confidence::High,
            matched_evidence: "GAH idle watchdog stall".to_string(),
            unresolved_timezone: None,
        }));
    }
    let Some(parsed) = crate::quota_parser::parse(backend, log_text, now) else {
        return Ok(None);
    };

    // Issue #437: ContextLimitExceeded is a routing signal, not a backend
    // availability problem. The same backend may succeed with a larger-context
    // model, so we must NOT mark it as unavailable.
    if parsed.kind == crate::quota_parser::FailureKind::ContextLimitExceeded {
        return Ok(Some(parsed));
    }
    // Output also carries the agent's own work, which can quote login-failure
    // text from the repository it is editing, and a login block never
    // expires. The backend's own login check overrules such a match.
    if parsed.kind == crate::quota_parser::FailureKind::AuthenticationError
        && crate::auth_health::login_confirmed(identity)
    {
        return Ok(None);
    }

    let parsed_unavailable_until = if let Some(reset_at) = parsed.reset_at.as_deref() {
        OffsetDateTime::parse(reset_at, &Rfc3339).ok()
    } else {
        parsed
            .retry_after_seconds
            .map(|secs| now + time::Duration::seconds(secs as i64))
    };
    // Issue #765: never trust a *computed* ETA past this ceiling -- a
    // mis-parsed or genuinely-wrong ETA observed live spanned multiple days
    // with zero self-correction (both codex and agy, neither of which are
    // `parsed.retryable` for their quota-exhausted classification -- that
    // field means "worth an immediate short-term retry," not "has no ETA",
    // so it's the wrong thing to gate on here). Only clamps when a real ETA
    // was actually computed: `None` (no `reset_at`/`retry_after_seconds` at
    // all, e.g. a bad-credentials auth error) is left untouched -- that
    // case genuinely needs a human, not a ceiling, and forcing it to
    // `Some(ceiling)` would just waste a retry against still-bad
    // credentials every tick. Capping here means every future write is
    // already safe; `availability::reprobe_stale_unavailable_records` (run
    // once per loop tick) separately corrects records written before this
    // existed.
    let ceiling =
        now + time::Duration::seconds(crate::availability::UNAVAILABLE_UNTIL_CEILING_SECONDS);
    let unavailable_until = parsed_unavailable_until.map(|until| until.min(ceiling));
    let reason = match parsed.kind {
        crate::quota_parser::FailureKind::QuotaExhausted => {
            crate::availability::Reason::QuotaExhausted
        }
        crate::quota_parser::FailureKind::RateLimited => crate::availability::Reason::RateLimited,
        crate::quota_parser::FailureKind::AuthenticationError => {
            crate::availability::Reason::AuthenticationError
        }
        crate::quota_parser::FailureKind::BackendStalled => {
            crate::availability::Reason::BackendOutage
        }
        // ContextLimitExceeded returned before availability handling.
        crate::quota_parser::FailureKind::ContextLimitExceeded => unreachable!(),
    };
    let summary = format!(
        "{}; confidence={:?}; log={}",
        parsed.matched_evidence, parsed.confidence, log_path
    );
    let transition = crate::availability::record_unavailable_for_identity(
        state_path,
        identity,
        reason,
        crate::availability::Source::BackendError,
        unavailable_until,
        Some(summary),
        now,
    )?;
    if transition && reason == crate::availability::Reason::QuotaExhausted {
        if let Some((cfg, profile)) = notify {
            crate::availability::notify_quota_exhausted(cfg, profile, identity);
        }
    }
    Ok(Some(parsed))
}

#[cfg(test)]
mod transition_tests {
    use super::*;

    #[test]
    fn quota_availability_notifies_once_across_retries_and_cooldown_probes() {
        let (tmp, cfg) = crate::ledger::test_util::test_config();
        let mut profile = crate::config::tests::test_profile_for_notifications();
        let notices = tmp.path().join("quota-notices.txt");
        profile.notify_command = Some(format!("cat >> {}", notices.display()));
        let path = tmp.path().join("availability.json");
        let identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
            "vibe",
            Some("mistral-glm-5-3"),
            None::<String>,
        );
        let output = include_str!("../../../tests/fixtures/quota-logs/vibe_budget_exhausted.txt");
        for _ in 0..20 {
            let failure = mark_failure(&path, &identity, output, "log", Some((&cfg, &profile)))
                .unwrap()
                .unwrap();
            assert_eq!(
                failure.kind,
                crate::quota_parser::FailureKind::QuotaExhausted
            );
        }
        assert_eq!(
            std::fs::read_to_string(&notices).unwrap().lines().count(),
            1
        );
        let now = crate::clock::now_utc();
        for tick in 0..20 {
            assert!(
                !crate::availability::availability_for_identity(
                    &path,
                    &identity,
                    now + time::Duration::seconds(tick * 5)
                )
                .unwrap()
                .eligible
            );
        }
        crate::availability::clear_quota_failure(&path, &identity, now).unwrap();
        mark_failure(&path, &identity, output, "log", Some((&cfg, &profile))).unwrap();
        assert_eq!(
            std::fs::read_to_string(&notices).unwrap().lines().count(),
            2
        );
    }

    #[test]
    fn quota_availability_terminal_runs_do_not_clear_on_a_zero_exit_rejection() {
        let (tmp, cfg) = crate::ledger::test_util::test_config();
        let path = tmp.path().join("availability.json");
        let _availability = crate::test_support::AvailabilityEnvGuard::set(&path);
        let mut profile = crate::config::tests::test_profile_for_notifications();
        let notices = tmp.path().join("notices");
        profile.notify_command = Some(format!("cat >> {}", notices.display()));
        let identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
            "vibe",
            Some("glm"),
            None::<String>,
        );
        let log = tmp.path().join("run.log");
        fs::write(&log, "budget exhausted").unwrap();
        let mut run = runner::RunResult {
            exit_code: 1,
            duration_secs: 0.0,
            log_path: log.display().to_string(),
            final_summary: None,
            agy_cli_log_delta: None,
            internal_log_delta: None,
            internal_log_path: None,
            transcript_path: None,
            agy_version: None,
            resources: Default::default(),
        };
        record_run_outcome(&cfg, &profile, &identity, &run).unwrap();
        record_run_outcome(&cfg, &profile, &identity, &run).unwrap();
        assert_eq!(fs::read_to_string(&notices).unwrap().lines().count(), 1);
        run.exit_code = 0;
        fs::write(&log, "").unwrap();
        run.internal_log_delta = Some("budget exhausted".into());
        record_run_outcome(&cfg, &profile, &identity, &run).unwrap();
        assert!(
            !crate::availability::availability_for_identity(
                &path,
                &identity,
                crate::clock::now_utc()
            )
            .unwrap()
            .eligible
        );
        run.internal_log_delta = None;
        record_run_outcome(&cfg, &profile, &identity, &run).unwrap();
        assert!(
            crate::availability::availability_for_identity(
                &path,
                &identity,
                crate::clock::now_utc()
            )
            .unwrap()
            .eligible
        );
        let review = runner::ReviewRunResult {
            outcome: runner::ReviewProcessOutcome::NonZeroExit(402),
            duration_secs: 0.0,
            stdout: String::new(),
            stderr: "budget exhausted".into(),
            idle_timeout_seconds: 60,
            hard_timeout_seconds: None,
            last_progress_secs: None,
            usage_artifact_path: None,
            agy_cli_log_delta: None,
            resources: Default::default(),
        };
        for _ in 0..20 {
            record_review_outcome(&cfg, &profile, &identity, &review).unwrap();
        }
        assert_eq!(fs::read_to_string(&notices).unwrap().lines().count(), 2);
    }
}
