//! Local-only host protocol. Descriptors 3/4 carry Bubblewrap options/status,
//! 5 carries trusted host receipts, and 6 is reserved for the capacity lease.
use anyhow::Result;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pub execution_id: uuid::Uuid,
    #[arg(long, value_parser = clap::value_parser!(u64).range(64..=1048576))]
    pub memory_mib: u64,
    #[arg(long)]
    pub bubblewrap: PathBuf,
    #[arg(long)]
    pub config: PathBuf,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..=86400))]
    pub timeout_seconds: u64,
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(2..=15))]
    pub heartbeat_seconds: u64,
    #[arg(long, value_parser = clap::value_parser!(u32).range(8..=1024))]
    pub max_processes: u32,
    #[arg(last = true, required = true)]
    pub command: Vec<String>,
}

#[cfg(target_os = "linux")]
#[path = "isolation_host/linux.rs"]
mod linux;

pub fn run(args: Args) -> Result<()> {
    #[cfg(target_os = "linux")]
    return linux::run(args);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        anyhow::bail!("Isolated host admission requires Linux.");
    }
}
