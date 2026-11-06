//! Fixed native job entry point. Secrets arrive only on bounded stdin and are
//! detached before any subprocess. This process consumes one start, installs
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

// Never derive Debug/Serialize: this envelope is not an artifact or diagnostic.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    grant: StartGrant,
    service_token: String,
}

fn parse_input(bytes: &[u8]) -> Result<Credentials> {
    ensure!(
        bytes.len() <= MAX_INPUT,
        "Native credential input exceeds its limit"
    );
    let input: Credentials = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Native credential input is invalid"))?;
    ensure!(
        !input.service_token.is_empty()
            && input.service_token.len() <= 4096
            && !input.service_token.chars().any(char::is_control)
            && input.grant.token.len() == 64
            && input.grant.token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Native credential input is invalid"
    );
    Ok(input)
}

fn read_input(file: &mut File) -> Result<Credentials> {
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
    parse_input(&bytes)
}

fn receive_input() -> Result<Credentials> {
    ensure!(
        !std::io::stdin().is_terminal(),
        "Native credentials require private noninteractive stdin"
    );
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
    let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
    ensure!(fd >= 0, "Native credential stdin is unavailable");
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = read_input(&mut file);
    drop(file);
    // A seekable stdin (e.g. a sealed memfd) must not be inherited by curl,
    // git, or a backend which could rewind and read the credential envelope.
    let null = File::open("/dev/null")?;
    ensure!(
        unsafe { libc::dup2(null.as_raw_fd(), libc::STDIN_FILENO) } >= 0,
        "Native credential stdin could not be detached"
    );
    result
}

/// Run only as the exact transient gah-native-job service corresponding to an
/// already-offered start. No arbitrary executable or authority origin is accepted.
/// A ledger is not proof of this wrapper's death: its parent must independently
/// observe service/cgroup termination before producing a terminal receipt.
pub fn run(
    context_dir: &Path,
    expected: &str,
    work: &Path,
    config_path: Option<&str>,
) -> Result<()> {
    let credentials = receive_input()?;
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
