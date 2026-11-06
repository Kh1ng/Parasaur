//! Small, bounded native control requests. Credentials and JSON travel only
//! through stdin. Kernel file limits bound capture even for chunked replies;
//! a timeout, protocol failure or oversized result never echoes remote data.
use super::*;
use anyhow::ensure;
use std::{os::unix::process::CommandExt, time::Duration};

const MAX_RESPONSE: usize = 64 * 1024;

/// POST one native control message with normal TLS validation and a fixed
/// five-second deadline. An absolute executable and disabled curl config/
/// proxy discovery keep profile environment changes out of credential routing.
pub(crate) fn request_control(url: &str, body: &str, token: &str) -> Result<CurlResponse> {
    let target = url::Url::parse(url)?;
    ensure!(
        url.len() <= 8192
            && matches!(target.scheme(), "https" | "http")
            && target.host_str().is_some()
            && target.username().is_empty()
            && target.password().is_none()
            && target.query().is_none()
            && target.fragment().is_none()
            && body.len() <= 16 * 1024
            && !token.is_empty()
            && token.len() <= 4096
            && !token.chars().any(char::is_control),
        "Invalid native control request"
    );
    let input=format!("silent\nurl = \"{}\"\nrequest = \"POST\"\nheader = \"Content-Type: application/json\"\nheader = \"Authorization: Bearer {}\"\ndata = \"{}\"\n",
        config_value(url),config_value(token),config_value(body));
    let mut command = Command::new("/usr/bin/curl");
    command.env_remove("SSLKEYLOGFILE");
    command.args([
        "-q",
        "-sS",
        "--noproxy",
        "*",
        "--proto",
        "=http,https",
        "--max-time",
        "5",
        "-K",
        "-",
        "-w",
        &format!("\n{STATUS_MARKER}%{{http_code}}\n"),
    ]);
    crate::runner::process::arm_child_pdeathsig(&mut command);
    // SAFETY: setrlimit is a direct Linux resource-limit syscall, with no
    // allocation or locks. run_bounded captures both streams in regular files;
    // a kernel-enforced cap also protects against an unknown response length.
    unsafe {
        command.pre_exec(|| {
            // Fail writes with EFBIG instead of dumping a credential-bearing
            // curl process through SIGXFSZ's default core action.
            if libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(std::io::Error::last_os_error());
            }
            let no_core = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &no_core) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let limit = libc::rlimit {
                rlim_cur: (MAX_RESPONSE + 128) as libc::rlim_t,
                rlim_max: (MAX_RESPONSE + 128) as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = crate::runner::process::run_bounded_with_input(
        command,
        Duration::from_secs(5),
        input.as_bytes(),
    )
    .context("Native control request timed out or could not be observed")?;
    ensure!(
        output.status.success() && output.stdout.len() <= MAX_RESPONSE + 128,
        "Native control transport failed or exceeded its response limit"
    );
    let response = split_status(output.stdout)
        .map_err(|_| anyhow::anyhow!("Invalid native control response"))?;
    ensure!(
        response.body.len() <= MAX_RESPONSE,
        "Native control response exceeds its limit"
    );
    Ok(response)
}

#[cfg(test)]
mod tests;
