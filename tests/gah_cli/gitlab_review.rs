use super::*;

#[test]
fn review_gitlab_posts_comment_and_keeps_ci_separate_from_mergeability() {
    let tmp = test_tempdir();
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    add_origin_and_feature_commit(&repo);
    checkout_branch(&repo, "main");
    let cfg = write_real_repo_config_with_extra(
        &tmp,
        &repo,
        "gitlab",
        "[profiles.real.routing]\nreview_backend = \"claude\"\n",
        "",
    );

    let fake_bin = tmp.path().join("bin");
    let glab_log = tmp.path().join("glab.log");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "claude",
        "#!/bin/sh\ncat <<'EOF'\nReview notes\n{\"verdict\":\"APPROVE\",\"confidence\":\"high\",\"human_required\":false,\"blocking_findings\":[],\"non_blocking_findings\":[\"Looks fine\"],\"risk_notes\":[\"low risk\"],\"evidence\":[\"file:src.txt\"]}\nEOF\n",
    );
    make_fake_bin_with_body(
        &fake_bin,
        "glab",
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
case "$2" in
  projects/42/merge_requests|projects/42/merge_requests\?*)
    printf '%s\n' '[{{"web_url":"https://gitlab.example.com/owner/real/-/merge_requests/7","iid":7,"source_branch":"feature/review","target_branch":"main","draft":true,"sha":"review-source-sha","detailed_merge_status":"draft","head_pipeline":{{"sha":"review-source-sha","status":"success"}}}}]' ;;
  projects/42/merge_requests/7/notes\?*) echo '[]' ;;
  projects/42/merge_requests/7/notes|projects/42/merge_requests/7)
    [ "$GITLAB_TOKEN" = bot-token ] && [ "$GLAB_TOKEN" = bot-token ] || exit 1
    cat >> '{log}'
    echo '{{"id":1}}' ;;
  *) exit 1 ;;
esac
"#,
            log = glab_log.display()
        ),
    );

    bin()
        .args([
            "dispatch",
            "--profile",
            "real",
            "--mode",
            "review",
            "--branch",
            "feature/review",
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("PATH", prepend_path(&fake_bin))
        .env("GITLAB_PAT", "bot-token")
        .env("GITLAB_TOKEN", "requester-token")
        .env("GLAB_TOKEN", "requester-token")
        .env_remove("GITLAB_PAT2")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());

    let glab_log = fs::read_to_string(glab_log).unwrap();
    assert!(glab_log.contains("source_branch=feature/review"));
    assert!(glab_log.contains("projects/42/merge_requests/7/notes"));
    assert!(glab_log.contains("\"add_labels\":\"gah-ready-for-human\""));
    assert!(!glab_log.contains("PRIVATE-TOKEN"));

    let session_dir = latest_child_dir(&tmp.path().join("artifacts/real/sessions"));
    let mr_description =
        fs::read_to_string(session_dir.join("review-bundle/mr-description.md")).unwrap();
    assert!(mr_description.contains("Draft: true"));
    assert!(mr_description.contains("CI: passed"));
    assert!(mr_description.contains("Mergeability: draft"));
}

#[test]
fn gitlab_fix_reads_threads_pushes_same_branch_and_replies_before_resolving() {
    let tmp = test_tempdir();
    let repo = tmp.path().join("repo");
    let home = tmp.path().join("home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&home).unwrap();
    init_git_repo(&repo);
    add_origin_and_feature_commit(&repo);
    checkout_branch(&repo, "main");
    configure_git_url_instead_of(
        &home,
        "https://oauth2@gitlab.example.com/owner/real.git",
        &format!("file://{}", tmp.path().join("origin.git").display()),
    );
    let cfg = write_real_repo_config_with_extra(&tmp,&repo,"gitlab",
        "validation_commands = [\"test -s src.txt\"]\n[profiles.real.routing]\nimprove_backend = \"claude\"\n[profiles.real.publishing]\ntrusted_issue_human_authors = [\"maintainer\"]\n", "");
    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let prompt = tmp.path().join("prompt.txt");
    let log = tmp.path().join("glab.log");
    let mut thread: Value =
        serde_json::from_str(include_str!("../fixtures/gitlab-19/discussions.json")).unwrap();
    thread[0]["notes"][0]["position"]["new_path"] = "src.txt".into();
    let thread = serde_json::to_string(&thread[0]).unwrap();
    fs::write(tmp.path().join("thread.json"), &thread).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "claude",
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$*" > '{prompt}'
echo checked > src.txt
echo '[{{"discussion_id":"abc123","summary":"Checked the return value","files":["src.txt"]}}]' > "$CARGO_TARGET_DIR/gitlab-addressed-threads.json"
echo 'Fixed the unchecked return and validated src.txt.'
"#,
            prompt = prompt.display()
        ),
    );
    make_fake_bin_with_body(
        &fake_bin,
        "glab",
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
sha=$(git --git-dir='{root}/origin.git' rev-parse refs/heads/feature/review)
mr='{{"iid":7,"web_url":"https://gitlab.example.com/owner/real/-/merge_requests/7","source_branch":"feature/review","target_branch":"main","state":"opened","title":"Review this","description":"","sha":"'"$sha"'","head_pipeline":{{"sha":"'"$sha"'","status":"success"}}}}'
case "$2" in
 projects/42/merge_requests|projects/42/merge_requests\?*) echo "[$mr]" ;;
 projects/42/merge_requests/7) echo "$mr" ;;
 projects/42/merge_requests/7/notes\?*) echo '[]' ;;
 projects/42/merge_requests/7/discussions\?*)
  [ "$GITLAB_TOKEN" = bot-token ] || exit 1
  printf '[%s]\n' "$(cat '{root}/thread.json')" ;;
 projects/42/merge_requests/7/discussions/abc123)
  [ "$GITLAB_TOKEN" = bot-token ] || exit 1
  if [ "$6" = PUT ]; then cat >> '{log}'; echo '{{}}'; else cat '{root}/thread.json'; fi ;;
 projects/42/merge_requests/7/discussions/abc123/notes)
  [ "$GITLAB_TOKEN" = bot-token ] || exit 1
  cat >> '{log}'; echo '{{"id":20}}' ;;
 *) echo '[]' ;;
esac
"#,
            root = tmp.path().display(),
            log = log.display()
        ),
    );
    bin()
        .args([
            "dispatch",
            "--profile",
            "real",
            "--mode",
            "fix",
            "--mr",
            "7",
            "--target",
            "fix review comments",
            "--skip-validation-gate",
            "--backend",
            "claude",
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("PATH", prepend_path(&fake_bin))
        .env("HOME", &home)
        .env("GITLAB_PAT", "bot-token")
        .env("GITLAB_TOKEN", "requester-token")
        .env("GLAB_TOKEN", "requester-token")
        .env_remove("GITLAB_PAT2")
        .assert()
        .success();
    let prompt = fs::read_to_string(prompt).unwrap();
    assert!(prompt.contains("unchecked return"));
    assert!(prompt.contains("gitlab-addressed-threads.json"));
    let log = fs::read_to_string(log).unwrap();
    let reply = log.find("Addressed in").expect("reply posted after push");
    let resolve = log.find("\"resolved\":true").expect("thread resolved");
    assert!(reply < resolve);
    let content = ProcessCommand::new("git")
        .args([
            "--git-dir",
            tmp.path().join("origin.git").to_str().unwrap(),
            "show",
            "feature/review:src.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&content.stdout).trim(), "checked");
}
