//! Fixed native job entry point. Secrets arrive on bounded stdin or a
//! PID-verified socket, closed before any subprocess. It consumes one start, installs
//! its verified scope, then runs the ordinary dispatch/ledger path exactly once.
use super::{worker::StartGrant, worker::WorkerRun};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::{
    fs::File,
    io::{IsTerminal, Read},
    os::fd::{AsRawFd, FromRawFd},
    path::Path,
    time::{Duration, Instant},
};

const MAX_INPUT: usize = 16 * 1024;

fn valid_owner(token: &str) -> bool {
    !token.is_empty() && token.len() <= 4096 && !token.chars().any(char::is_control)
}

// Never derive Debug/Serialize: this envelope is not an artifact or diagnostic.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Credentials {
    pub(super) grant: StartGrant,
    pub(super) service_token: String,
}

fn parse_input(bytes: &[u8]) -> Result<Credentials> {
    ensure!(
        bytes.len() <= MAX_INPUT,
        "Native credential input exceeds its limit"
    );
    let input: Credentials = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Native credential input is invalid"))?;
    ensure!(
        valid_owner(&input.service_token)
            && input.grant.token.len() == 64
            && input.grant.token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Native credential input is invalid"
    );
    Ok(input)
}

fn read_bytes(file: &mut File) -> Result<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "Native credential input timed out");
        let mut event = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut event, 1, remaining.as_millis().max(1) as i32) };
        if ready < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        ensure!(
            ready > 0,
            "Native credential input is unavailable or timed out"
        );
        let mut buffer = [0; 4096];
        let count = file
            .read(&mut buffer)
            .context("Native credential input is unavailable")?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        ensure!(
            bytes.len() <= MAX_INPUT,
            "Native credential input exceeds its limit"
        );
    }
    Ok(bytes)
}

fn read_input(file: &mut File) -> Result<Credentials> {
    parse_input(&read_bytes(file)?)
}

fn protect_memory() -> Result<()> {
    // Disable wrapper core dumps and /proc memory access before reading a
    // credential. The owner token must not end up in a crash artifact.
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    ensure!(
        unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } == 0
            && unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } == 0,
        "Native credential memory protection is unavailable"
    );
    Ok(())
}

pub(super) fn receive_input() -> Result<Credentials> {
    parse_input(&receive_bytes()?)
}

pub(crate) fn receive_owner() -> Result<String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Owner {
        service_token: String,
    }
    let input: Owner = serde_json::from_slice(&receive_bytes()?)
        .map_err(|_| anyhow::anyhow!("Native service credential input is invalid"))?;
    ensure!(
        valid_owner(&input.service_token),
        "Native service credential input is invalid"
    );
    Ok(input.service_token)
}

pub(super) fn receive_bytes() -> Result<Vec<u8>> {
    ensure!(
        !std::io::stdin().is_terminal(),
        "Native credentials require private noninteractive stdin"
    );
    protect_memory()?;
    let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
    ensure!(fd >= 0, "Native credential stdin is unavailable");
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = read_bytes(&mut file);
    drop(file);
    // A seekable stdin (e.g. a sealed memfd) must not be inherited by curl,
    // git, or a backend which could rewind and read the credential envelope.
    detach_input()?;
    result
}

fn detach_input() -> Result<()> {
    let null = File::open("/dev/null")?;
    ensure!(
        unsafe { libc::dup2(null.as_raw_fd(), libc::STDIN_FILENO) } >= 0,
        "Native credential stdin could not be detached"
    );
    Ok(())
}

/// Reverify staged input and reject unsupported provider claim lifecycles.
pub(super) fn prepare(
    context_dir: &Path,
    expected: &str,
    work: &Path,
    config_path: Option<&str>,
) -> Result<super::WorkerContext> {
    let worker = crate::config::load(config_path)?;
    let node = crate::central_claims::resolve_node_id()?;
    let context = super::resume(&worker, context_dir, expected, &node, work)?;
    // A background provider-assignee lease has its own write/renewal lifecycle.
    // Until that lifecycle is integrated with native operation fences, do not
    // silently run it outside those fences or weaken the provider claim policy.
    let profile = crate::config::get_profile(&context.config, &context.binding.profile)?;
    ensure!(
        profile.publishing.issue_claim.mode == crate::config::IssueClaimMode::Local,
        "Native provider-assignee claim integration is unavailable; retain this job for reconciliation"
    );
    Ok(context)
}

/// Enter the exact transient job for an already-offered start. Its parent must
/// independently observe termination before producing a terminal receipt.
pub fn run(
    context_dir: &Path,
    expected: &str,
    work: &Path,
    config_path: Option<&str>,
    socket: Option<&Path>,
) -> Result<()> {
    let credentials = if let Some(path) = socket {
        protect_memory()?;
        detach_input()?;
        let mut file = super::credential_socket::connect(path)?;
        read_input(&mut file)?
    } else {
        receive_input()?
    };
    let context = prepare(context_dir, expected, work, config_path)?;
    {
        let mut journal = WorkerRun::open(work, &context.binding)?;
        journal.consume_start(&context, &credentials.grant, &credentials.service_token)?;
    }
    let cfg = context.config.clone();
    let args = context.request.args.clone();
    super::execution::install(context, work.into(), credentials.service_token)?;
    crate::dispatch::run(&cfg, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_credential_pipe_is_bounded() {
        let mut descriptors = [-1; 2];
        assert_eq!(
            unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) },
            0
        );
        let mut reader = unsafe { File::from_raw_fd(descriptors[0]) };
        let _writer = unsafe { File::from_raw_fd(descriptors[1]) };
        let started = Instant::now();
        assert!(read_input(&mut reader)
            .err()
            .unwrap()
            .to_string()
            .contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn credential_schema_is_bounded_strict_and_never_echoed() {
        let private = "SECRET_FIXTURE_CANARY";
        for bytes in [
            private.as_bytes().to_vec(),
            serde_json::to_vec(&serde_json::json!({"service_token": private, "command":"sh"}))
                .unwrap(),
            vec![b'x'; MAX_INPUT + 1],
        ] {
            let error = parse_input(&bytes).err().unwrap().to_string();
            assert!(!error.contains(private));
        }
        let mut value = serde_json::json!({"grant":{
            "run_id":"fixture", "node_id":"worker", "context_sha256":"a".repeat(64),
            "reservation_id":"b".repeat(32), "expires_at":"fixture", "token":"c".repeat(64)
        },"service_token":private});
        assert!(parse_input(&serde_json::to_vec(&value).unwrap()).is_ok());
        value["service_token"] = "bad\ncredential".into();
        assert!(parse_input(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
