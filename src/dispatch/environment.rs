use crate::config::Profile;
use crate::runner;

/// Exports `profile.env_file` (or `env_file_prod` with `--prod`) into the
/// real process environment, as early as possible.
///
/// `profile.pat()` and other provider.rs calls (GitLab/GitHub API lookups
/// made by the harness itself -- MR creation, review-target resolution,
/// posting comments) read GITLAB_PAT/GITHUB_TOKEN etc. via `std::env::var`
/// directly, and those calls can happen before any backend is spawned.
/// Loading the env file into a `Vec<(String, String)>` for a spawned
/// child's environment (done later, per mode, for the backend process
/// itself) never reaches these in-process calls -- confirmed live: a
/// review dispatch failed 3 layers downstream with a git refspec error
/// because GITLAB_PAT was never actually in this process's environment.
pub(in crate::dispatch) fn export_profile_env(profile: &Profile, prod: bool) {
    let resolved_env = profile_env_file(profile, prod);
    if resolved_env.is_empty() {
        return;
    }
    let settings = runner::load_env_file(resolved_env);
    for (key, value) in &settings {
        if is_reserved(key) {
            continue;
        }
        adopt_current_name(key, value, &settings);
        std::env::set_var(key, value);
    }
}

/// Gives a setting the file names the old way its current name in this
/// process, which is the only name the code reads. Startup adoption ran before
/// the file was loaded, so the current name is unset or still holds a copy of
/// the old name's inherited value; either would hide the file's value. A
/// current name the file sets itself, or one that was set on its own account,
/// is left alone.
///
/// This touches only this process's environment. The list handed to backend
/// and reviewer processes comes from the parser unchanged, so credential
/// filtering by declared name still sees exactly the names in the file.
fn adopt_current_name(key: &str, value: &str, settings: &[(String, String)]) {
    let Some(current) = crate::legacy_settings::current_name(key) else {
        return;
    };
    let in_file = settings.iter().any(|(other, _)| *other == current);
    let copied = std::env::var_os(&current).is_none_or(|held| Some(held) == std::env::var_os(key));
    if !in_file && copied && !is_reserved(&current) {
        std::env::set_var(current, value);
    }
}

/// The env file a dispatch loads: `env_file_prod` with `--prod`, otherwise
/// `env_file`. Empty when the profile sets none.
pub(in crate::dispatch) fn profile_env_file(profile: &Profile, prod: bool) -> &str {
    let file = if prod {
        &profile.env_file_prod
    } else {
        &profile.env_file
    };
    file.as_deref().unwrap_or("")
}

/// Keys a profile env file may not set. `PARASAUR_ENFORCE_JOB_FILE` carries the
/// `gah dispatch --enforce-job-file` opt-in, which decides whether commands
/// from a job file are executed (issue #1474): only that flag may set it, so
/// an env file can neither switch enforcement on for every direct dispatch
/// nor silently switch the flag off.
fn is_reserved(key: &str) -> bool {
    const OPT_IN: &str = "PARASAUR_ENFORCE_JOB_FILE";
    key == OPT_IN || crate::legacy_settings::legacy_name(OPT_IN).as_deref() == Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_file_cannot_set_the_job_file_opt_in() {
        let _exec_guard = crate::test_support::ExecGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        let env_file = tmp.path().join("profile.env");
        std::fs::write(
            &env_file,
            "PARASAUR_ENFORCE_JOB_FILE=1\nPARASAUR_TEST_1474_CONTROL=exported\n",
        )
        .unwrap();
        let mut profile = crate::dispatch::test_util::profile(tmp.path());
        profile.env_file = Some(env_file.to_string_lossy().into_owned());
        let before = std::env::var("PARASAUR_ENFORCE_JOB_FILE").ok();
        std::env::remove_var("PARASAUR_TEST_1474_CONTROL");

        export_profile_env(&profile, false);

        assert_eq!(std::env::var("PARASAUR_ENFORCE_JOB_FILE").ok(), before);
        assert_eq!(
            std::env::var("PARASAUR_TEST_1474_CONTROL").as_deref(),
            Ok("exported"),
            "ordinary keys are still exported"
        );
        std::env::remove_var("PARASAUR_TEST_1474_CONTROL");
    }

    #[test]
    fn an_old_name_in_the_env_file_reaches_the_current_name_in_this_process_only() {
        let _exec_guard = crate::test_support::ExecGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        let env_file = tmp.path().join("profile.env");
        let mut profile = crate::dispatch::test_util::profile(tmp.path());
        profile.env_file = Some(env_file.to_string_lossy().into_owned());
        const OLD: &str = "GAH_TEST_ENV_FILE_SETTING";
        const CURRENT: &str = "PARASAUR_TEST_ENV_FILE_SETTING";
        let current = || std::env::var(CURRENT).ok();
        let opt_in = std::env::var("PARASAUR_ENFORCE_JOB_FILE").ok();
        let load = |file: &str, inherited_old: Option<&str>, inherited_current: Option<&str>| {
            std::fs::write(&env_file, file).unwrap();
            for (name, value) in [(OLD, inherited_old), (CURRENT, inherited_current)] {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
            export_profile_env(&profile, false);
        };
        let old_only = format!("{OLD}=file\nGAH_ENFORCE_JOB_FILE=1\n");

        load(&old_only, None, None);
        assert_eq!(current().as_deref(), Some("file"));
        // What startup adoption leaves when the old name was inherited.
        load(&old_only, Some("inherited"), Some("inherited"));
        assert_eq!(current().as_deref(), Some("file"));
        load(&old_only, Some("inherited"), Some("chosen"));
        assert_eq!(
            current().as_deref(),
            Some("chosen"),
            "set on its own account"
        );
        load(
            &format!("{OLD}=file\n{CURRENT}=newer\n"),
            Some("inherited"),
            Some("inherited"),
        );
        assert_eq!(current().as_deref(), Some("newer"));

        // The opt-in is refused under both spellings.
        assert_eq!(std::env::var("PARASAUR_ENFORCE_JOB_FILE").ok(), opt_in);
        assert_eq!(std::env::var("GAH_ENFORCE_JOB_FILE").ok(), None);
        // Child processes are given the parsed file, which gains no names:
        // credential filtering matches the names an operator declared.
        let names: Vec<String> = runner::load_env_file(&profile.env_file.clone().unwrap())
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, [OLD, CURRENT]);
        std::env::remove_var(OLD);
        std::env::remove_var(CURRENT);
    }
}
