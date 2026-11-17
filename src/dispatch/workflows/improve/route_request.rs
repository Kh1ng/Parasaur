use crate::config::{self, GahConfig};
use crate::dispatch::{issues::TicketMetadata, DispatchArgs};
use crate::ledger;
use crate::routing::RouteRequest;
use std::path::Path;

/// Bind operator selections and ticket hints to the initial route and every
/// validation retry without changing the chosen account.
pub(super) fn for_dispatch<'a>(
    cfg: &GahConfig,
    args: &'a DispatchArgs,
    ticket_meta: Option<&'a TicketMetadata>,
    session_dir: &'a Path,
) -> RouteRequest<'a> {
    let usage_summary = ledger::usage_summary_for_backend(
        cfg,
        args.backend.as_str(),
        args.model.as_deref(),
        Some(
            session_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(""),
        ),
    )
    .ok();
    RouteRequest {
        mode: &args.mode,
        requested_backend: config::canonical_backend_name(&args.backend),
        requested_instance: args.backend_instance.as_deref(),
        requested_model: args.model.as_deref(),
        recommended_backend: ticket_meta.and_then(|m| m.recommended_backend.as_deref()),
        recommended_model: ticket_meta.and_then(|m| m.recommended_model.as_deref()),
        session_id: session_dir.file_name().and_then(|s| s.to_str()),
        usage_summary,
        last_failure_class: if args.escalate {
            Some(crate::ledger::FailureClass::AgentNoProgress.as_str())
        } else {
            None
        },
        exact_route_required: false,
    }
}
