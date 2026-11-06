//! Credential delivery bound to the launched main PID. A different process,
//! even under the same Unix account, cannot read the service owner's envelope.
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::net::{UnixListener, UnixStream},
    },
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn connect(path: &Path) -> Result<File> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    ensure!(
        bytes.len() < address.sun_path.len() && !bytes.contains(&0),
        "Native credential socket path is invalid"
    );
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (output, input) in address.sun_path.iter_mut().zip(bytes) {
        *output = *input as libc::c_char;
    }
    let descriptor = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    ensure!(descriptor >= 0, "Native credential socket is unavailable");
    let file = unsafe { File::from_raw_fd(descriptor) };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let result = unsafe {
            libc::connect(
                descriptor,
                &address as *const _ as *const libc::sockaddr,
                (std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1) as libc::socklen_t,
            )
        };
        if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EISCONN) {
            return Ok(file);
        }
        ensure!(
            matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR | libc::EAGAIN | libc::EINPROGRESS | libc::EALREADY)
            ),
            "Native credential connection failed"
        );
        ensure!(
            Instant::now() < deadline,
            "Native credential connection timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn is_peer(stream: &UnixStream, pid: u32) -> Result<bool> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut credentials as *mut _ as *mut libc::c_void,
                &mut size,
            )
        } == 0
            && size == std::mem::size_of::<libc::ucred>() as libc::socklen_t,
        "Native credential peer identity is unavailable"
    );
    Ok(credentials.pid > 0
        && credentials.pid as u32 == pid
        && credentials.uid == unsafe { libc::geteuid() })
}

pub(super) fn deliver(
    listener: &UnixListener,
    pid: u32,
    bytes: &[u8],
    current: impl Fn() -> Result<()>,
) -> Result<()> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        ensure!(
            Instant::now() < deadline,
            "Native credential delivery timed out; reconcile this launch"
        );
        match listener.accept() {
            Ok((mut stream, _)) => {
                if !is_peer(&stream, pid)? {
                    continue;
                }
                current()?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                stream
                    .write_all(bytes)
                    .context("Native credential delivery failed; reconcile this launch")?;
                stream.shutdown(std::net::Shutdown::Write)?;
                return Ok(());
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(_) => anyhow::bail!("Native credential delivery failed; reconcile this launch"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_user_wrong_pid_cannot_read_credential_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input");
        let listener = UnixListener::bind(&path).unwrap();
        let mut intruder = UnixStream::connect(&path).unwrap();
        assert!(deliver(
            &listener,
            std::process::id() + 1,
            b"PRIVATE_CREDENTIAL_CANARY",
            || panic!("Wrong peer reached service check")
        )
        .is_err());
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut intruder, &mut bytes).unwrap();
        assert!(bytes.is_empty());
    }
}
