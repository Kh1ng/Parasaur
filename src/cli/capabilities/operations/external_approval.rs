//! `gah external-approval` operations for the capability manifest.

use super::*;

pub(in crate::cli::capabilities) fn add_external_approval_operations(
    manifest: &mut CapabilityManifest,
) {
    // gah external-approval list
    manifest.add_operation(OperationDefinition {
        operation_id: "external_approval.list".to_string(),
        display_name: "List External Approvals".to_string(),
        class: OperationClass::Read,
        profile_scope: ProfileScope::ProfileRequired,
        request_schema: None,
        response_schema: None,
        streaming: StreamingBehavior::None,
        idempotency: Idempotency::Idempotent,
        secret_fields: vec![],
        remote_disposition: RemoteDisposition::RemoteAvailable,
        local_only_reason: None,
        documentation: Some("List external approval scopes for a profile".to_string()),
        cli_command_path: "gah external-approval list".to_string(),
        is_stable: true,
    });

    // gah external-approval request
    manifest.add_operation(OperationDefinition {
        operation_id: "external_approval.request".to_string(),
        display_name: "Request External Approval".to_string(),
        class: OperationClass::Mutation,
        profile_scope: ProfileScope::ProfileRequired,
        request_schema: None,
        response_schema: None,
        streaming: StreamingBehavior::None,
        idempotency: Idempotency::Idempotent,
        secret_fields: vec![
            SecretFieldSpec {
                field_path: "credential_label".to_string(),
                is_secret: false,
                may_contain_secrets: true,
            },
            SecretFieldSpec {
                field_path: "max_dollars".to_string(),
                is_secret: false,
                may_contain_secrets: false,
            },
        ],
        remote_disposition: RemoteDisposition::RemoteAvailable,
        local_only_reason: None,
        documentation: Some("Record a requested external operation approval for a specific profile/repo/work item and credential label".to_string()),
        cli_command_path: "gah external-approval request".to_string(),
        is_stable: true,
    });

    // gah external-approval inspect
    manifest.add_operation(OperationDefinition {
        operation_id: "external_approval.inspect".to_string(),
        display_name: "Inspect External Approval".to_string(),
        class: OperationClass::Read,
        profile_scope: ProfileScope::ProfileRequired,
        request_schema: None,
        response_schema: None,
        streaming: StreamingBehavior::None,
        idempotency: Idempotency::Idempotent,
        secret_fields: vec![],
        remote_disposition: RemoteDisposition::RemoteAvailable,
        local_only_reason: None,
        documentation: Some(
            "Inspect the current external approval state for one exact scope".to_string(),
        ),
        cli_command_path: "gah external-approval inspect".to_string(),
        is_stable: true,
    });

    // gah external-approval grant
    manifest.add_operation(OperationDefinition {
        operation_id: "external_approval.grant".to_string(),
        display_name: "Grant External Approval".to_string(),
        class: OperationClass::Mutation,
        profile_scope: ProfileScope::ProfileRequired,
        request_schema: None,
        response_schema: None,
        streaming: StreamingBehavior::None,
        idempotency: Idempotency::Idempotent,
        secret_fields: vec![],
        remote_disposition: RemoteDisposition::RemoteAvailable,
        local_only_reason: None,
        documentation: Some("Grant the requested external approval scope".to_string()),
        cli_command_path: "gah external-approval grant".to_string(),
        is_stable: true,
    });

    // gah external-approval deny
    manifest.add_operation(OperationDefinition {
        operation_id: "external_approval.deny".to_string(),
        display_name: "Deny External Approval".to_string(),
        class: OperationClass::Mutation,
        profile_scope: ProfileScope::ProfileRequired,
        request_schema: None,
        response_schema: None,
        streaming: StreamingBehavior::None,
        idempotency: Idempotency::Idempotent,
        secret_fields: vec![],
        remote_disposition: RemoteDisposition::RemoteAvailable,
        local_only_reason: None,
        documentation: Some("Deny a pending external approval request".to_string()),
        cli_command_path: "gah external-approval deny".to_string(),
        is_stable: true,
    });

    // gah external-approval revoke
    manifest.add_operation(OperationDefinition {
        operation_id: "external_approval.revoke".to_string(),
        display_name: "Revoke External Approval".to_string(),
        class: OperationClass::Mutation,
        profile_scope: ProfileScope::ProfileRequired,
        request_schema: None,
        response_schema: None,
        streaming: StreamingBehavior::None,
        idempotency: Idempotency::Idempotent,
        secret_fields: vec![],
        remote_disposition: RemoteDisposition::RemoteAvailable,
        local_only_reason: None,
        documentation: Some("Revoke an active external approval".to_string()),
        cli_command_path: "gah external-approval revoke".to_string(),
        is_stable: true,
    });

    // gah external-approval expire
    manifest.add_operation(OperationDefinition {
        operation_id: "external_approval.expire".to_string(),
        display_name: "Expire External Approval".to_string(),
        class: OperationClass::Mutation,
        profile_scope: ProfileScope::ProfileRequired,
        request_schema: None,
        response_schema: None,
        streaming: StreamingBehavior::None,
        idempotency: Idempotency::Idempotent,
        secret_fields: vec![],
        remote_disposition: RemoteDisposition::RemoteAvailable,
        local_only_reason: None,
        documentation: Some("Expire an external approval immediately".to_string()),
        cli_command_path: "gah external-approval expire".to_string(),
        is_stable: true,
    });
}
