use super::*;

#[test]
fn production_approval_requires_exact_changed_file_and_control_plane_ci() {
    let json = r#"{"verdict":"Approve","confidence":"high","human_required":false,"blocking_findings":[],"non_blocking_findings":[],"risk_notes":[],"evidence":["file:not-in-diff.rs","ci:passed"]}"#;
    let usage = crate::ledger::LedgerUsage::default();
    let route = route_decision("claude", Some("sonnet"), false);
    let context = ReviewGateContext::from_diff_bundle(
        &ReviewDiffBundle {
            files: "src/dispatch.rs\n".to_string(),
            diff: "+fn hardened_review() {}\n".to_string(),
        },
        Some("passed"),
    );

    let verdict =
        parse_review_verdict_with_context(json, &route, &usage, ReviewerTier::Strong, &context)
            .unwrap();

    assert_eq!(verdict.verdict, "HUMAN_REVIEW");
    assert!(verdict
        .safety_gate_reason
        .as_deref()
        .unwrap_or_default()
        .contains("not grounded"));
}

#[test]
fn production_approval_does_not_require_ci_before_review() {
    let json = r#"{"verdict":"APPROVE","confidence":"high","human_required":false,"blocking_findings":[],"non_blocking_findings":[],"risk_notes":[],"evidence":["file:src/internal.rs"]}"#;
    let usage = crate::ledger::LedgerUsage::default();
    let route = route_decision("claude", Some("sonnet"), false);
    let context = ReviewGateContext::from_diff_bundle(
        &ReviewDiffBundle {
            files: "src/internal.rs\n".to_string(),
            diff: "+fn internal_only() {}\n".to_string(),
        },
        Some("pending"),
    );

    let verdict =
        parse_review_verdict_with_context(json, &route, &usage, ReviewerTier::Strong, &context)
            .unwrap();

    assert_eq!(verdict.verdict, "APPROVE");
    assert!(!verdict.human_required);
}

#[test]
fn production_approval_cannot_falsely_claim_ci_passed_before_ci_finishes() {
    let json = r#"{"verdict":"APPROVE","confidence":"high","human_required":false,"blocking_findings":[],"non_blocking_findings":[],"risk_notes":[],"evidence":["file:src/internal.rs","ci:passed"]}"#;
    let usage = crate::ledger::LedgerUsage::default();
    let route = route_decision("claude", Some("sonnet"), false);
    let context = ReviewGateContext::from_diff_bundle(
        &ReviewDiffBundle {
            files: "src/internal.rs\n".to_string(),
            diff: "+fn internal_only() {}\n".to_string(),
        },
        Some("pending"),
    );

    let verdict =
        parse_review_verdict_with_context(json, &route, &usage, ReviewerTier::Strong, &context)
            .unwrap();

    assert_eq!(verdict.verdict, "HUMAN_REVIEW");
    assert!(verdict
        .safety_gate_reason
        .as_deref()
        .unwrap_or_default()
        .contains("claimed passed CI"));
}

#[test]
fn production_approval_with_prose_is_held_to_prevent_hidden_findings() {
    let review_text = "Found a worrying edge case.\n{\"verdict\":\"APPROVE\",\"confidence\":\"high\",\"human_required\":false,\"blocking_findings\":[],\"non_blocking_findings\":[],\"risk_notes\":[],\"evidence\":[\"file:src/dispatch.rs\",\"ci:passed\"]}";
    let usage = crate::ledger::LedgerUsage::default();
    let route = route_decision("claude", Some("sonnet"), false);
    let context = ReviewGateContext::from_diff_bundle(
        &ReviewDiffBundle {
            files: "src/dispatch.rs\n".to_string(),
            diff: "+fn hardened_review() {}\n".to_string(),
        },
        Some("passed"),
    );

    let verdict = parse_review_verdict_with_context(
        review_text,
        &route,
        &usage,
        ReviewerTier::Strong,
        &context,
    )
    .unwrap();

    assert_eq!(verdict.verdict, "HUMAN_REVIEW");
    assert!(verdict
        .safety_gate_reason
        .as_deref()
        .unwrap_or_default()
        .contains("substantive prose"));
}
