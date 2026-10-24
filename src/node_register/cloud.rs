use super::{read_or_create_identity, resolve_identity_path, RegisterOptions};
use crate::config::{GahConfig, OrchestrationMode};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloudState {
    Healthy,
    Configuration(&'static str),
    Identity,
    Transport,
    Http(u16),
}

#[derive(Default)]
struct CloudRegistration {
    registered: Option<[u8; 32]>,
    state: Option<CloudState>,
    failures: u64,
}

struct Connection {
    url: Url,
    anon_key: String,
    access_token: String,
    workspace_id: String,
    device_token: Option<String>,
}

impl Connection {
    fn from_env(env: &impl Fn(&str) -> Option<String>) -> Result<Self, CloudState> {
        let required = |name| {
            env(name)
                .filter(|value| !value.trim().is_empty() && !value.contains(['\r', '\n']))
                .ok_or(CloudState::Configuration(name))
        };
        let mut url = Url::parse(&required("GAH_SUPABASE_URL")?)
            .map_err(|_| CloudState::Configuration("GAH_SUPABASE_URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(CloudState::Configuration("GAH_SUPABASE_URL"));
        }
        url.set_path(&format!(
            "{}/rest/v1/rpc/",
            url.path().trim_end_matches('/')
        ));
        let workspace_id = required("GAH_SUPABASE_WORKSPACE_ID")?;
        if uuid::Uuid::parse_str(&workspace_id).is_err() {
            return Err(CloudState::Configuration("GAH_SUPABASE_WORKSPACE_ID"));
        }
        Ok(Self {
            url,
            workspace_id,
            anon_key: required("GAH_SUPABASE_ANON_KEY")?,
            access_token: required("GAH_SUPABASE_ACCESS_TOKEN")?,
            device_token: env("GAH_SUPABASE_DEVICE_TOKEN").filter(|value| !value.trim().is_empty()),
        })
    }

    fn rpc(&self, function: &str, body: &serde_json::Value, token: &str) -> Result<(), CloudState> {
        let url = self.url.join(function).expect("fixed RPC path");
        let response = crate::curl_http::request_with_headers(
            "POST",
            url.as_str(),
            Some(&body.to_string()),
            Some(token),
            3,
            &[("apikey", &self.anon_key)],
        )
        .map_err(|_| CloudState::Transport)?;
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            // Responses and curl errors can contain credentials; log only the status.
            Err(CloudState::Http(response.status))
        }
    }
}

impl CloudRegistration {
    fn sync(
        &mut self,
        cfg: &GahConfig,
        opts: Option<&RegisterOptions>,
        env: impl Fn(&str) -> Option<String>,
    ) -> bool {
        if cfg.defaults.orchestration_mode == OrchestrationMode::Local {
            *self = Self::default();
            return false;
        }
        let result = self.send(cfg, opts, &env);
        let next = result.err().unwrap_or(CloudState::Healthy);
        if next != CloudState::Healthy {
            self.failures = self.failures.saturating_add(1);
        }
        let changed = self.state != Some(next);
        if changed {
            match next {
                CloudState::Healthy if self.state.is_some() => eprintln!(
                    "gah: cloud node registration recovered ({} failed attempts)",
                    self.failures
                ),
                CloudState::Healthy => {}
                failure => eprintln!(
                    "gah: WARNING cloud node registration degraded: {failure:?} \
                     ({} failed attempts); continuing local operation",
                    self.failures
                ),
            }
            self.state = Some(next);
        }
        changed
    }

    fn send(
        &mut self,
        cfg: &GahConfig,
        opts: Option<&RegisterOptions>,
        env: &impl Fn(&str) -> Option<String>,
    ) -> Result<(), CloudState> {
        let connection = Connection::from_env(env)?;
        let identity_path = opts
            .and_then(|opts| opts.identity_path.clone())
            .or_else(|| env("GAH_COORDINATOR_IDENTITY_PATH").map(Into::into))
            .unwrap_or_else(resolve_identity_path);
        let advertised_url = opts
            .and_then(|opts| opts.advertised_url.clone())
            .or_else(|| env("GAH_NODE_ADVERTISED_URL"));
        let identity = read_or_create_identity(&identity_path, advertised_url.as_deref())
            .map_err(|_| CloudState::Identity)?;
        let profiles = opts
            .and_then(|opts| opts.profiles.clone())
            .unwrap_or_else(|| {
                let mut profiles: Vec<_> = cfg.profiles.keys().cloned().collect();
                profiles.sort_unstable();
                profiles
            });
        let transport_mode = opts
            .map(|opts| opts.transport_mode.clone())
            .or_else(|| env("GAH_REGISTRY_TRANSPORT_MODE"))
            .unwrap_or_else(|| "trusted_lan".into());
        let payload = serde_json::json!({
            "p_workspace_id": connection.workspace_id,
            "p_node_id": identity.node_id,
            "p_display_name": identity.display_name,
            "p_advertised_url": advertised_url.unwrap_or(identity.advertised_url),
            "p_version": identity.version,
            "p_schema_digest": identity.schema_digest,
            "p_transport_mode": transport_mode,
            "p_profiles": profiles,
        });
        // Reconcile again when identity, profiles, endpoint, or credentials change.
        let mut fingerprint = Sha256::new();
        for value in [
            payload.to_string(),
            connection.url.to_string(),
            connection.anon_key.clone(),
            connection.access_token.clone(),
            connection.device_token.clone().unwrap_or_default(),
        ] {
            fingerprint.update(value.len().to_le_bytes());
            fingerprint.update(value.as_bytes());
        }
        let fingerprint = fingerprint.finalize().into();
        if self.registered != Some(fingerprint) {
            connection.rpc("register_cloud_node", &payload, &connection.access_token)?;
            self.registered = Some(fingerprint);
        }
        let device_token = connection
            .device_token
            .as_deref()
            .ok_or(CloudState::Configuration("GAH_SUPABASE_DEVICE_TOKEN"))?;
        connection.rpc(
            "cloud_node_heartbeat",
            &serde_json::json!({"p_device_token": device_token}),
            &connection.anon_key,
        )
    }
}

/// Best-effort cloud sink for `gah node register` and each existing controller
/// pass, including startup. Local mode does not read cloud credentials or identity.
/// POST `{GAH_SUPABASE_URL}/rest/v1/rpc/register_cloud_node` upserts with a user
/// JWT; POST `.../rest/v1/rpc/cloud_node_heartbeat` renews the device's 90s lease.
/// Both send `apikey` and `Authorization: Bearer` headers. Requests time out after
/// three seconds each. Failed upserts retry next pass; repeated failures of the
/// same kind warn once. The sink counts failed attempts during each cloud session.
/// No service-role key is read. See `docs/cloud-node-registration.md` for inputs.
pub fn cloud_advisory(cfg: &GahConfig, opts: Option<&RegisterOptions>) {
    static CLOUD: Mutex<CloudRegistration> = Mutex::new(CloudRegistration {
        registered: None,
        state: None,
        failures: 0,
    });
    // Another in-process caller may already be syncing. Never wait on that caller.
    if let Ok(mut cloud) = CLOUD.try_lock() {
        cloud.sync(cfg, opts, |name| std::env::var(name).ok());
    }
}

#[cfg(test)]
mod tests;
