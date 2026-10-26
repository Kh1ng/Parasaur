use super::*;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;

struct Github {
    dir: tempfile::TempDir,
    profile: Profile,
}

impl Github {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = crate::config::tests::test_profile_for_notifications();
        profile.provider = "github".into();
        profile.repo = "owner/repo".into();
        let script = r#"#!/bin/sh
case "$1 $2 $3 $4" in
  "api --method GET repos/owner/repo/pulls") echo '[{"number":42}]' ;;
  "pr view 42 --repo") echo '{"number":42,"title":"test","body":"body","headRefOid":"H","isDraft":false}' ;;
  "pr ready 42 --repo" | "pr merge 42 --squash") echo "$*" >> "${0%/*}/mutations" ;;
  "api --method GET "*)
    file="${4##*/}"
    case "$4" in
      */issues/42/comments*) file="issue-$file" ;;
    esac
    /bin/cat "${0%/*}/$file.json" ;;
  *) echo "unexpected gh invocation: $*" >&2; exit 1 ;;
esac
"#;
        let path = dir.path().join("gh");
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        crate::provider::set_test_provider_path(dir.path().to_str().unwrap());
        let github = Self { dir, profile };
        for endpoint in ["issue-comments", "comments", "reviews", "timeline"] {
            github.page(endpoint, 1, json!([]));
        }
        github
    }

    fn page(&self, endpoint: &str, page: usize, value: Value) {
        fs::write(
            self.dir
                .path()
                .join(format!("{endpoint}?per_page=100&page={page}.json")),
            value.to_string(),
        )
        .unwrap();
    }

    fn blockers(&self, head: &str) -> Vec<String> {
        blockers(&self.profile, "42", head).unwrap()
    }
}

impl Drop for Github {
    fn drop(&mut self) {
        crate::provider::clear_test_provider_path();
    }
}

fn comment(id: u64, login: &str, verdict: &str, head: Option<&str>) -> Value {
    let mut body = format!("GAH review verdict: `{verdict}`\n");
    if let Some(head) = head {
        body.push_str(&format!(
            "\n<!-- gah-review-head-v1 {} -->\n",
            json!({"head":head,"backend":"codex","model":null})
        ));
    }
    json!({"id":id,"user":{"login":login},"body":body,"created_at":format!("2026-10-25T00:00:{id:02}Z")})
}

#[test]
fn foreign_verdict_blocks_merge_until_same_login_approves_or_head_changes() {
    let _exec = crate::test_support::ExecGuard::new();
    let github = Github::new();
    let veto = comment(1, "foreign", "NEEDS_FIX", Some("H"));
    github.page(
        "issue-comments",
        1,
        json!([
            comment(4, "foreign", "APPROVE", Some("old")),
            veto,
            comment(2, "own", "APPROVE", Some("H"))
        ]),
    );
    assert_eq!(github.blockers("H"), ["foreign"]);
    let error = crate::provider::merge_mr(&github.profile, "gah/test", None).unwrap_err();
    assert!(error.to_string().contains("veto from foreign"), "{error:#}");
    assert!(!github.dir.path().join("mutations").exists());
    assert!(github.blockers("H-prime").is_empty());

    // Cross-resource and case-insensitive login identity; input is deliberately unsorted.
    github.page(
        "comments",
        1,
        json!([comment(3, "FOREIGN", "APPROVE", Some("H"))]),
    );
    assert!(github.blockers("H").is_empty());
    crate::provider::merge_mr(&github.profile, "gah/test", None).unwrap();
    assert!(fs::read_to_string(github.dir.path().join("mutations"))
        .unwrap()
        .contains("pr merge"));
}

#[test]
fn own_verdicts_human_review_and_review_bodies_obey_the_same_gate() {
    let _exec = crate::test_support::ExecGuard::new();
    let github = Github::new();
    for verdict in ["NEEDS_FIX", "HUMAN_REVIEW", "REJECT"] {
        let veto = comment(1, "own", verdict, Some("H"));
        github.page(
            "reviews",
            1,
            json!([{
                "id":1,"user":{"login":"own"},"state":"COMMENTED",
                "body":veto["body"],"commit_id":"H","submitted_at":veto["created_at"]
            }]),
        );
        assert_eq!(github.blockers("H"), ["own"]);
        github.page(
            "issue-comments",
            1,
            json!([comment(2, "own", "APPROVE", Some("H"))]),
        );
        assert!(github.blockers("H").is_empty());
        github.page("issue-comments", 1, json!([]));
    }
}

#[test]
fn real_changes_requested_reviews_block_from_any_login_on_their_head() {
    let _exec = crate::test_support::ExecGuard::new();
    let github = Github::new();
    let request = json!({"id":1,"user":{"login":"untrusted"},"state":"CHANGES_REQUESTED","commit_id":"H","submitted_at":"2026-10-25T00:00:01Z"});
    github.page("reviews", 1, json!([request]));
    assert_eq!(github.blockers("H"), ["untrusted"]);
    assert!(github.blockers("new").is_empty());
    // A newer approval on an older head must not withdraw the current request.
    let approval = json!({"id":2,"user":{"login":"untrusted"},"state":"APPROVED","commit_id":"old","submitted_at":"2026-10-25T00:00:02Z"});
    github.page("reviews", 1, json!([request, approval]));
    assert_eq!(github.blockers("H"), ["untrusted"]);
    github.page(
        "issue-comments",
        1,
        json!([comment(3, "untrusted", "APPROVE", Some("H"))]),
    );
    assert!(github.blockers("H").is_empty());
    github.page("issue-comments", 1, json!([]));
    for state in ["APPROVED", "DISMISSED"] {
        let mut cleared = request.clone();
        cleared["id"] = json!(2);
        cleared["state"] = json!(state);
        cleared["submitted_at"] = json!("2026-10-25T00:00:02Z");
        github.page("reviews", 1, json!([request, cleared]));
        assert!(github.blockers("H").is_empty());
    }
    github.page(
        "issue-comments",
        1,
        json!([comment(3, "untrusted", "NEEDS_FIX", Some("H"))]),
    );
    assert_eq!(github.blockers("H"), ["untrusted"]);
}

#[test]
fn legacy_verdicts_use_push_order_for_issue_and_inline_comments() {
    let _exec = crate::test_support::ExecGuard::new();
    let github = Github::new();
    github.page(
        "issue-comments",
        1,
        json!([comment(1, "issue", "NEEDS_FIX", None)]),
    );
    let mut inline = comment(2, "inline", "HUMAN_REVIEW", None);
    inline["pull_request_review_id"] = json!(10);
    github.page("comments", 1, json!([inline]));
    for push in ["committed", "head_ref_force_pushed", "head_ref_restored"] {
        let mut timeline = json!([
            {"event":push}, {"event":"commented","id":1}, {"event":"reviewed","id":10}
        ]);
        github.page("timeline", 1, timeline.clone());
        assert_eq!(github.blockers("H"), ["inline", "issue"]);
        timeline.as_array_mut().unwrap().push(json!({"event":push}));
        github.page("timeline", 1, timeline);
        assert!(github.blockers("H-prime").is_empty());
    }
}

#[test]
fn verdict_gate_reads_all_pages_and_fails_closed_on_read_or_receipt_errors() {
    let _exec = crate::test_support::ExecGuard::new();
    let github = Github::new();
    for endpoint in ["issue-comments", "comments", "reviews"] {
        let padding = if endpoint == "reviews" {
            json!({"id":0,"state":"COMMENTED","body":"unrelated"})
        } else {
            json!({"id":0,"body":"unrelated","user":null,"created_at":""})
        };
        github.page(endpoint, 1, json!(vec![padding; 100]));
        let veto = comment(1, "later-page", "NEEDS_FIX", Some("H"));
        let second = if endpoint == "reviews" {
            json!([{"id":1,"state":"COMMENTED","body":veto["body"],"user":{"login":"later-page"},"submitted_at":veto["created_at"]}])
        } else {
            json!([veto])
        };
        github.page(endpoint, 2, second);
        assert_eq!(github.blockers("H"), ["later-page"]);
        github.page(endpoint, 1, json!([]));
    }
    github.page("comments", 1, json!([{"id":2,"user":{"login":"foreign"},"created_at":"now","body":"GAH review verdict: `NEEDS_FIX`\n<!-- gah-review-head-v1 broken -->"}]));
    assert!(blockers(&github.profile, "42", "H").is_err());
    fs::remove_file(github.dir.path().join("comments?per_page=100&page=1.json")).unwrap();
    assert!(blockers(&github.profile, "42", "H").is_err());
}

#[test]
fn verdict_parser_does_not_import_quoted_or_unknown_protocol_states() {
    for body in [
        "quoted\nGAH review verdict: `NEEDS_FIX`",
        "GAH review verdict: `NOT_APPROVE`",
        "GAH review verdict: `APPROVE` extra",
    ] {
        assert_eq!(Verdict::parse(body), None);
    }
    assert_eq!(
        Verdict::parse("GAH review verdict: NEEDS_FIX"),
        Some(Verdict::NeedsFix)
    );
}
