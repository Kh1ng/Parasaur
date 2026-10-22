//! `gah diag export`: download this machine's diagnostics bundle (memory
//! timeline, host and version details) from the local Parasaur server.
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::io::Write;
use std::path::Path;

#[derive(Args)]
pub struct DiagArgs {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Write the diagnostics bundle (.json.gz) to send to whoever is debugging.
    Export {
        /// Output file (default: ./parasaur-diagnostics-<time>.json.gz).
        #[arg(long)]
        out: Option<String>,
        /// Server to read from (default: GAH_SERVER_URL or http://127.0.0.1:3773).
        #[arg(long)]
        server: Option<String>,
    },
}

pub fn run(args: DiagArgs) -> Result<()> {
    let Action::Export { out, server } = args.action;
    let server = server
        .or_else(|| std::env::var("GAH_SERVER_URL").ok())
        .unwrap_or_else(|| crate::mcp::DEFAULT_SERVER_URL.to_string());
    let token = std::env::var("GAH_SERVER_TOKEN").ok();
    let url = format!("{}/api/diagnostics/export", server.trim_end_matches('/'));
    let response = crate::curl_http::request("GET", &url, None, token.as_deref(), 60)
        .with_context(|| format!("requesting {url}; is the Parasaur server running?"))?;
    if response.status != 200 {
        bail!(
            "{url} returned HTTP {}: {}",
            response.status,
            String::from_utf8_lossy(&response.body).trim()
        );
    }
    // A success page that is not gzip (a proxy or an older server) is not a
    // bundle; refuse it rather than save something nobody can open.
    if !response.body.starts_with(&[0x1f, 0x8b]) {
        bail!("{url} returned HTTP 200 but not a gzip diagnostics bundle");
    }
    let out = out.unwrap_or_else(|| {
        format!(
            "parasaur-diagnostics-{}.json.gz",
            chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ")
        )
    });
    write_replacing(Path::new(&out), &response.body).with_context(|| format!("writing {out}"))?;
    println!("Wrote {out} ({} KiB)", response.body.len() / 1024);
    Ok(())
}

/// Stages the bundle beside `path` and renames it into place, so a failed
/// write never leaves a truncated file where an earlier export was.
fn write_replacing(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}
