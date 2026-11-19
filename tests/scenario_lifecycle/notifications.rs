use super::{green, harness, needs_fix, publish_head, reviewer, run, start, ScenarioHarness};
use crate::support::scenario::central::FakeCentral;
use serde_json::{json, Value};
use std::fs;

fn webhook(harness: ScenarioHarness, receiver: &FakeCentral) -> ScenarioHarness {
    fs::write(
        harness.artifacts_dir.join("notify.env"),
        format!("DISCORD_WEBHOOK_URL={}/test/webhook\n", receiver.url),
    )
    .unwrap();
    harness.with_defaults_append("notifications_enabled = true\nnotification_channel = \"discord\"")
}

fn messages(receiver: &FakeCentral) -> Vec<String> {
    receiver.state()["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| {
            assert_eq!(call["method"], "POST");
            assert_eq!(call["target"], "/test/webhook");
            let content = call["body"]["content"].as_str().unwrap();
            assert_eq!(call["body"], json!({"content":content}));
            content.to_owned()
        })
        .collect()
}

#[test]
fn create_review_and_merge_each_notify_once() {
    let receiver = FakeCentral::new();
    let (mut harness, _) = run(webhook(harness(), &receiver), "dispatch_ticket", 0);
    publish_head(&mut harness, "2");
    green(&mut harness, "2");
    reviewer(&mut harness, "APPROVE");
    let (harness, _) = run(harness, "review_mr", 0);
    let (harness, _) = run(harness, "mark_ready_for_review", 0);
    let (harness, _) = run(harness, "merge_mr", 0);
    let _ = run(harness, "no_op", 0);
    let received = messages(&receiver);
    let url = "https://github.com/owner/repo/pull/2";
    assert_eq!(
        received,
        [
            format!("[gah] MR created {url} (work_id=#1, agy/test-model)"),
            format!("[gah] review APPROVE on {url}"),
            format!("[gah] auto-merged {url} (work_id=#1)"),
        ]
    );
}

#[test]
fn terminal_dispatch_failure_and_human_gate_notify_once_across_ticks() {
    let (harness, _) = needs_fix(start());
    let receiver = FakeCentral::new();
    let harness = webhook(harness, &receiver);
    crate::support::write_executable(
        &harness.bin_dir.join("agy"),
        "#!/bin/sh\necho repair failed >&2\nexit 23\n",
    );
    let (harness, first) = run(harness, "fix_mr", 1);
    let (harness, _) = run(harness, "fix_mr", 1);
    let (harness, _) = run(harness, "human_required", 0);
    let _ = run(harness, "human_required", 0);
    let received = messages(&receiver);
    assert_eq!(received.len(), 2, "{received:?}");
    let failure = first
        .events
        .iter()
        .find(|e| e["event_type"] == "terminal_failure")
        .unwrap();
    let failure: Value = serde_json::from_str(failure["details"].as_str().unwrap()).unwrap();
    let run_id = failure["run_id"].as_str().unwrap();
    assert!(!run_id.is_empty());
    for field in [
        "dispatch terminal failure",
        "class=backend_error",
        "work_id=#1",
        run_id,
    ] {
        assert!(received[0].contains(field), "{}", received[0]);
    }
    for field in [
        "human required",
        "fix_retry_cap_exceeded",
        "https://github.com/owner/repo/pull/2",
    ] {
        assert!(received[1].contains(field), "{}", received[1]);
    }
}

#[test]
fn unavailable_webhook_retries_then_records_failure_and_review_continues() {
    let receiver = FakeCentral::new();
    receiver.update(|s| s["webhook_replies"] = json!([503, 503]));
    let (mut harness, created) = run(webhook(harness(), &receiver), "dispatch_ticket", 0);
    let attempts = messages(&receiver);
    assert_eq!(attempts.len(), 2, "{attempts:?}");
    assert_eq!(attempts[0], attempts[1]);
    let errors: Vec<_> = created
        .events
        .iter()
        .filter(|e| e["event_type"] == "notification_delivery_failed")
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    let details: Value = serde_json::from_str(errors[0]["details"].as_str().unwrap()).unwrap();
    assert_eq!(details["event_name"], "mr_created");
    assert_eq!(details["channel"], "discord");
    assert!(details["error"].as_str().unwrap().contains("503"));
    publish_head(&mut harness, "2");
    green(&mut harness, "2");
    reviewer(&mut harness, "APPROVE");
    receiver.update(|s| s["webhook_replies"] = json!([503, 200]));
    let (_, reviewed) = run(harness, "review_mr", 0);
    let received = messages(&receiver);
    assert_eq!(received.len(), 4, "{received:?}");
    assert!(received[2].contains("review APPROVE"));
    assert_eq!(received[2], received[3]);
    assert_eq!(
        reviewed
            .events
            .iter()
            .filter(|e| e["event_type"] == "notification_delivery_failed")
            .count(),
        1
    );
}
