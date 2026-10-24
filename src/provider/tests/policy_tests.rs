use super::*;

#[test]
fn review_and_mark_ready_refused_when_policy_read_only() {
    let _exec = crate::test_support::ExecGuard::new();
    let tmp = TempDir::new().unwrap();
    let calls = tmp.path().join("calls");
    for bin in ["gh", "glab"] {
        make_fake_bin(
            tmp.path(),
            bin,
            &format!("#!/bin/sh\necho called >> '{}'\nexit 1\n", calls.display()),
        );
    }
    let _path = PathOverride::set(tmp.path().display().to_string());
    for mut profile in [github_profile(), gitlab_profile()] {
        profile.policy_path = Some(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/policy-read-only.toml"
            )
            .into(),
        );
        let error =
            super::super::post_review_comment(&profile, "gah/test", "review", &[]).unwrap_err();
        assert!(crate::policy::policy_blocked(&error).is_some(), "{error:#}");
        let error = mark_ready_for_review(&profile, "gah/test").unwrap_err();
        assert!(crate::policy::policy_blocked(&error).is_some(), "{error:#}");
    }
    assert!(
        !calls.exists(),
        "a policy refusal must not call the provider"
    );
}
