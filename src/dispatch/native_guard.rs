//! Native job-only dispatch gates. Ordinary invocations retain their existing
//! execution path; serialized requests cannot create process authority.
use super::*;
use crate::{execution_identity::ExecutionIdentity, runner};

pub(crate) fn active() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::native_handoff::execution::active()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

pub(crate) fn verify(_cfg: &GahConfig, _args: &DispatchArgs) -> Result<()> {
    #[cfg(target_os = "linux")]
    crate::native_handoff::execution::verify_dispatch(_cfg, _args)?;
    Ok(())
}

pub(crate) fn verify_identity(_identity: &ExecutionIdentity) -> Result<()> {
    #[cfg(target_os = "linux")]
    crate::native_handoff::execution::verify_identity(_identity)?;
    Ok(())
}

pub(crate) fn verify_environment() -> Result<()> {
    #[cfg(target_os = "linux")]
    crate::native_handoff::execution::verify_environment()?;
    Ok(())
}

pub(crate) fn authority_held(_error: &anyhow::Error) -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::native_handoff::execution::authority_held(_error)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

pub(crate) fn attempt(
    _identity: &ExecutionIdentity,
    invoke: impl FnOnce() -> Result<runner::RunResult>,
) -> Result<runner::RunResult> {
    #[cfg(target_os = "linux")]
    {
        crate::native_handoff::execution::attempt(_identity, invoke)
    }
    #[cfg(not(target_os = "linux"))]
    {
        invoke()
    }
}

pub(crate) fn review(
    _identity: &ExecutionIdentity,
    invoke: impl FnOnce() -> Result<runner::ReviewRunResult>,
) -> Result<runner::ReviewRunResult> {
    #[cfg(target_os = "linux")]
    {
        crate::native_handoff::execution::review(_identity, invoke)
    }
    #[cfg(not(target_os = "linux"))]
    {
        invoke()
    }
}

pub(crate) fn publication<T>(
    _profile: &crate::config::Profile,
    invoke: impl FnOnce() -> Result<T>,
) -> Result<T> {
    #[cfg(target_os = "linux")]
    {
        crate::native_handoff::execution::publication(_profile, invoke)
    }
    #[cfg(not(target_os = "linux"))]
    {
        invoke()
    }
}

pub(crate) fn finish() -> Result<()> {
    #[cfg(target_os = "linux")]
    crate::native_handoff::execution::finish_dispatch()?;
    Ok(())
}

pub(crate) fn publication_observed<T>(
    _profile: &crate::config::Profile,
    invoke: impl FnOnce() -> Result<T>,
    _confirmed: impl FnOnce(&T) -> bool,
) -> Result<T> {
    #[cfg(target_os = "linux")]
    {
        crate::native_handoff::execution::publication_observed(_profile, invoke, _confirmed)
    }
    #[cfg(not(target_os = "linux"))]
    {
        invoke()
    }
}
