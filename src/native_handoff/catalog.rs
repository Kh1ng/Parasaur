//! Secret-free execution-node observations. They replace only coordinator-local
//! executable checks, never configured permissions, approvals or spend history.
use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCatalog {
    pub protocol: u32,
    pub node_id: String,
    pub profile: String,
    pub repo_id: String,
    pub repo: String,
    pub instances: Vec<WorkerInstance>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerInstance {
    pub backend_instance: String,
    pub logical_backend: String,
    pub enabled: bool,
    pub executable_ready: bool,
    pub auth_ready: Option<bool>,
    pub supported_models: Vec<String>,
}

impl WorkerCatalog {
    pub fn read(path: &Path) -> Result<Self> {
        read_json(&read_blob(path, None, MAX_JSON)?)
    }

    pub(crate) fn apply(
        &self,
        profile_name: &str,
        profile: &crate::config::Profile,
        node_id: &str,
        runtime: &mut crate::routing::RoutingRuntimeState,
    ) -> Result<()> {
        let bounded = |value: &str| {
            !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        };
        ensure!(
            self.protocol == 1
                && self.node_id == node_id
                && bounded(node_id)
                && self.profile == profile_name
                && self.repo_id == profile.repo_id
                && self.repo == profile.repo
                && self.instances.len() <= 128,
            "Worker catalog identity or protocol changed"
        );
        let mut seen = HashSet::new();
        let mut available = HashMap::new();
        for instance in &self.instances {
            ensure!(
                bounded(&instance.backend_instance)
                    && bounded(&instance.logical_backend)
                    && seen.insert(instance.backend_instance.clone())
                    && instance.supported_models.len() <= 256
                    && instance.supported_models.iter().all(|model| bounded(model)),
                "Invalid worker catalog instance"
            );
            if instance.enabled && instance.executable_ready && instance.auth_ready != Some(false) {
                available.insert(
                    instance.backend_instance.clone(),
                    crate::routing::RemoteInstanceAvailability {
                        backend: instance.logical_backend.clone(),
                        models: instance.supported_models.clone(),
                    },
                );
            }
        }
        runtime.remote_instances = Some(available);
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCheckScope {
    catalog: WorkerCatalog,
    held_work: Vec<crate::central_claims::NativeHeldWork>,
}

impl WorkerCheckScope {
    pub fn read(path: &Path) -> Result<Self> {
        read_json(&read_blob(path, None, MAX_JSON)?)
    }
    pub fn node_id(&self) -> &str {
        &self.catalog.node_id
    }
    pub fn check_current(
        &self,
        cfg: &GahConfig,
        context: &Path,
        expected: &str,
        origin: &str,
        require_next_action: bool,
    ) -> Result<DispatchBinding> {
        super::work::check_current(
            cfg,
            context,
            expected,
            &self.catalog.node_id,
            origin,
            require_next_action,
            Some(self),
        )
    }
    pub(super) fn catalog(&self) -> &WorkerCatalog {
        &self.catalog
    }
    pub(super) fn project(
        &self,
        profile: &str,
        bound: &DispatchBinding,
        snapshot: Option<&mut crate::status::StatusSnapshot>,
    ) -> Result<()> {
        ensure!(
            self.held_work.iter().any(|hold| hold.profile == profile
                && hold.run_id == bound.run_id
                && hold.node_id == bound.node_id
                && hold.context_sha256 == bound.context_sha256
                && crate::work_claim::normalize_work_identity(&hold.work_id)
                    == crate::work_claim::normalize_work_identity(&bound.work_id)),
            "Original native ownership is absent from central scope"
        );
        crate::controller::project_native_holds(
            profile,
            snapshot,
            &self.held_work,
            Some(&bound.run_id),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog(cfg: &GahConfig) -> WorkerCatalog {
        let profile = &cfg.profiles["test"];
        WorkerCatalog {
            protocol: 1,
            node_id: "worker".into(),
            profile: "test".into(),
            repo_id: profile.repo_id.clone(),
            repo: profile.repo.clone(),
            instances: vec![WorkerInstance {
                backend_instance: "work".into(),
                logical_backend: "codex".into(),
                enabled: true,
                executable_ready: true,
                auth_ready: None,
                supported_models: vec!["fixture-model".into()],
            }],
        }
    }
    #[test]
    fn native_plan_uses_the_workers_account_without_a_coordinator_executable() {
        let _exec = crate::test_support::ExecGuard::new();
        let (directory, mut cfg, _worker, mut request) = super::super::tests::fixture();
        let _availability = crate::test_support::AvailabilityEnvGuard::set(
            directory.path().join("availability.json"),
        );
        let profile = cfg.profiles.get_mut("test").unwrap();
        profile.routing.improve_candidates.as_mut().unwrap()[0].instance = Some("work".into());
        request.args.backend = "auto".into();
        request.args.backend_instance = None;
        request.args.model = None;
        let planned = crate::dispatch::plan_native(&cfg, &request, &catalog(&cfg)).unwrap();
        assert_eq!(planned.args.backend, "codex");
        assert_eq!(planned.args.backend_instance.as_deref(), Some("work"));
        assert_eq!(planned.args.model.as_deref(), Some("fixture-model"));
        assert_eq!(planned.args.run_id, request.args.run_id);
    }
    #[test]
    fn unavailable_worker_accounts_cannot_borrow_a_coordinators_executable_or_other_model() {
        let _exec = crate::test_support::ExecGuard::new();
        let (directory, mut cfg, _worker, request) = super::super::tests::fixture();
        let _availability = crate::test_support::AvailabilityEnvGuard::set(
            directory.path().join("availability.json"),
        );
        let executable = directory.path().join("provider");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        cfg.profiles
            .get_mut("test")
            .unwrap()
            .routing
            .backend_instances
            .get_mut("work")
            .unwrap()
            .executable = Some(executable.to_string_lossy().into_owned());
        for change in 0..4 {
            let mut unavailable = catalog(&cfg);
            match change {
                0 => unavailable.instances[0].enabled = false,
                1 => unavailable.instances[0].executable_ready = false,
                2 => unavailable.instances[0].auth_ready = Some(false),
                _ => unavailable.instances[0].supported_models = vec!["other-model".into()],
            }
            assert!(
                crate::dispatch::plan_native(&cfg, &request, &unavailable).is_err(),
                "accepted unavailable account case {change}"
            );
        }
    }
    #[test]
    fn catalog_identity_and_canonical_ownership_scope_are_independent_requirements() {
        let (_directory, cfg, _worker, request) = super::super::tests::fixture();
        let mut wrong = catalog(&cfg);
        wrong.repo = "other/repo".into();
        assert!(wrong
            .apply(
                "test",
                &cfg.profiles["test"],
                "worker",
                &mut Default::default()
            )
            .is_err());
        let portable = super::super::policy::prepare(&cfg, "test").unwrap();
        let bound = super::super::binding(&request, "worker", &portable, &"a".repeat(64)).unwrap();
        let mut scope = WorkerCheckScope {
            catalog: catalog(&cfg),
            held_work: vec![],
        };
        assert!(scope.project("test", &bound, None).is_err());
        scope.held_work.push(crate::central_claims::NativeHeldWork {
            run_id: bound.run_id.clone(),
            node_id: bound.node_id.clone(),
            context_sha256: bound.context_sha256.clone(),
            profile: "test".into(),
            work_id: bound.work_id.clone(),
            reserves_intake: true,
        });
        scope.project("test", &bound, None).unwrap();
        scope.held_work[0].context_sha256 = "b".repeat(64);
        assert!(scope.project("test", &bound, None).is_err());
    }
}
