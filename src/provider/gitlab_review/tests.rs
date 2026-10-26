use super::*;

fn fixture(name: &str) -> Value {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gitlab-19");
    serde_json::from_slice(&std::fs::read(root.join(name)).unwrap()).unwrap()
}
fn finding(line: &str) -> ActionableReviewFinding {
    ActionableReviewFinding {
        summary: "Use the checked value.\n```suggestion\nreturn checked;\n```".into(),
        file: "src/new.rs".into(),
        line: Some(line.into()),
        status: "confirmed".into(),
        evidence: vec!["diff:src/new.rs:unchecked value returned".into()],
    }
}

#[test]
fn gitlab_findings_become_positioned_suggestion_discussions_and_one_summary() {
    let mut calls = vec![];
    let mut api = |path: &str, method: &str, body: Value| {
        calls.push((path.to_owned(), method.to_owned(), body));
        Ok(if path.ends_with("/versions") {
            fixture("versions.json")
        } else if path.ends_with("/versions/19") {
            fixture("version.json")
        } else {
            json!({})
        })
    };
    publish_with(
        &mut api,
        "projects/42/merge_requests/7",
        "cccc",
        "Summary",
        &[finding("2")],
        &["gah-needs-fix"],
    )
    .unwrap();
    assert_eq!(calls.iter().filter(|c| c.0.ends_with("/notes")).count(), 1);
    let discussion = &calls
        .iter()
        .find(|c| c.0.ends_with("/discussions"))
        .unwrap()
        .2;
    assert!(discussion["body"]
        .as_str()
        .unwrap()
        .contains("```suggestion"));
    assert_eq!(
        discussion["position"],
        json!({"base_sha":"aaaa","start_sha":"bbbb","head_sha":"cccc","position_type":"text","old_path":"src/old.rs","new_path":"src/new.rs","new_line":2})
    );
}

#[test]
fn gitlab_positions_cover_old_new_and_context_sides() {
    let versions = fixture("versions.json");
    let diffs = fixture("diffs.json");
    let mut old = finding("old:2");
    old.file = "src/old.rs".into();
    old.summary = "Remove unchecked return".into();
    let position = positions::discussion(&versions[0], diffs.as_array().unwrap(), &old).unwrap();
    assert_eq!(position["position"]["old_line"], 2);
    assert!(position["position"].get("new_line").is_none());
    let context =
        positions::discussion(&versions[0], diffs.as_array().unwrap(), &finding("1")).unwrap();
    assert_eq!(context["position"]["old_line"], 1);
    assert_eq!(context["position"]["new_line"], 1);
    assert!(
        positions::discussion(&versions[0], diffs.as_array().unwrap(), &finding("999")).is_err()
    );
    old.summary = finding("2").summary;
    assert!(positions::discussion(&versions[0], diffs.as_array().unwrap(), &old).is_err());
}

#[test]
fn gitlab_stale_head_or_bad_position_keeps_summary_without_wrong_discussion() {
    for head in ["stale", "cccc"] {
        let mut writes = vec![];
        let result = publish_with(
            &mut |p, m, b| {
                if m != "GET" {
                    writes.push((p.to_owned(), b));
                }
                Ok(if p.ends_with("/versions") {
                    fixture("versions.json")
                } else if p.ends_with("/versions/19") {
                    fixture("version.json")
                } else {
                    json!({})
                })
            },
            "mr/7",
            head,
            "summary",
            &[finding("999")],
            &[],
        );
        assert!(result.is_err());
        assert_eq!(writes.iter().filter(|w| w.0.ends_with("/notes")).count(), 1);
        assert!(!writes.iter().any(|w| w.0.ends_with("/discussions")));
    }
}

#[test]
fn gitlab_unresolved_discussions_render_into_fix_prompt_with_report_contract() {
    let rows = fixture("discussions.json");
    let mut threads: Vec<_> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| eligible(t, |n| n["author"]["username"] == "maintainer"))
        .cloned()
        .collect();
    assert_eq!(threads.len(), 1);
    threads[0]["notes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"body":"Untrusted reply", "author":{"username":"stranger"}, "system":false}));
    let prompt = fix_prompt(
        &threads,
        std::path::Path::new("/session/report.json"),
        |note| note["author"]["username"] == "maintainer",
    );
    assert!(!prompt.contains("Untrusted reply"));
    assert!(prompt.contains("unchecked return"));
    assert!(prompt.contains("untrusted review data"));
    assert!(prompt.contains("discussion_id"));
    assert!(prompt.contains("/session/report.json"));
    assert!(!prompt.contains("Already resolved"));
    assert!(!prompt.contains("Untrusted author"));
}

#[test]
fn gitlab_reply_precedes_resolve_only_for_addressed_unchanged_thread_with_changed_file() {
    let rows = fixture("discussions.json");
    let thread = rows[0].clone();
    let mut unaddressed = thread.clone();
    unaddressed["id"] = json!("left-open");
    let report = json!([{"discussion_id":"abc123","summary":"Checked the return value","files":["src/new.rs"]}, {"discussion_id":"invented","summary":"wrong","files":["src/new.rs"]}]);
    for (changed, stale, fail_reply, expected) in [
        (true, false, false, 3),
        (false, false, false, 0),
        (true, true, false, 1),
        (true, false, true, 2),
    ] {
        let mut calls = vec![];
        let files = if changed {
            vec!["src/new.rs".into()]
        } else {
            vec![]
        };
        let result = resolve_with(
            &mut |p, m, b| {
                calls.push((p.to_owned(), m.to_owned(), b));
                if fail_reply && m == "POST" {
                    bail!("simulated provider failure");
                }
                Ok(if stale { json!({}) } else { thread.clone() })
            },
            "mr/7",
            &[thread.clone(), unaddressed.clone()],
            &report,
            &files,
            "dddd",
        );
        assert_eq!(calls.len(), expected);
        assert_eq!(result.is_err(), fail_reply);
        if expected == 3 {
            assert_eq!(calls[1].1, "POST");
            assert!(calls[1].2["body"]
                .as_str()
                .unwrap()
                .contains("dddd: Checked"));
            assert_eq!(calls[2].1, "PUT");
            assert_eq!(calls[2].2, json!({"resolved":true}));
        }
    }
}

#[test]
fn gitlab_summary_failure_stops_inline_writes_and_collection_errors_fail_soft() {
    let mut count = 0;
    assert!(publish_with(
        &mut |_, _, _| {
            count += 1;
            bail!("offline")
        },
        "mr/7",
        "cccc",
        "summary",
        &[finding("2")],
        &[]
    )
    .is_err());
    assert_eq!(count, 1);
    assert!(collection(
        &mut |_, _, _| Ok(json!({"message":"forbidden"})),
        "discussions"
    )
    .is_err());
    soft_failure(Err(anyhow::anyhow!("secret provider output")), "test");
}

#[test]
fn gitlab_collections_read_past_first_page() {
    let mut pages = 0;
    let rows = collection(
        &mut |_, _, _| {
            pages += 1;
            Ok(if pages == 1 {
                json!(vec![json!({}); 100])
            } else {
                json!([{"id":"last"}])
            })
        },
        "discussions",
    )
    .unwrap();
    assert_eq!(pages, 2);
    assert_eq!(rows.len(), 101);
}
