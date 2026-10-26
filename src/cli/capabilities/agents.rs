use super::*;

pub(super) fn add_operations(manifest: &mut CapabilityManifest) {
    for (action, class, idempotency) in [
        (
            "check-update-policy",
            OperationClass::Read,
            Idempotency::Idempotent,
        ),
        ("versions", OperationClass::Read, Idempotency::Idempotent),
        ("update", OperationClass::Admin, Idempotency::NonIdempotent),
    ] {
        manifest.add_operation(OperationDefinition {
            operation_id: format!("agents.{action}"),
            display_name: format!("Agent CLI {action}"),
            class,
            profile_scope: ProfileScope::Global,
            request_schema: None,
            response_schema: None,
            streaming: StreamingBehavior::None,
            idempotency,
            secret_fields: vec![],
            remote_disposition: if action == "check-update-policy" {
                RemoteDisposition::LocalOnly
            } else {
                RemoteDisposition::RemoteAvailable
            },
            local_only_reason: if action == "check-update-policy" {
                Some(LocalOnlyReason::FilesystemAccessRequired)
            } else {
                None
            },
            documentation: Some("Node-local agent CLI inventory and owner-approved updates".into()),
            cli_command_path: format!("gah agents {action}"),
            is_stable: true,
        });
    }
    manifest.set_request_schema(
        "agents.versions",
        serde_json::json!({"type": "object", "properties": {}}),
    );
    manifest.set_request_schema("agents.update", serde_json::json!({
        "type": "object",
        "properties": {
            "backend": {"type": "string", "enum": ["claude", "codex", "agy", "vibe", "opencode"], "description": "Installed agent CLI to update."},
            "node_id": {"type": "string", "description": "Registered node ID; omit for the local node."}
        },
        "required": ["backend"]
    }));
}
