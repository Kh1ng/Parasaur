//! Private native context preparation and verification. These commands never
//! start a backend or treat a point-in-time history snapshot as live authority.
use anyhow::{ensure, Result};
use std::{io::Read, path::PathBuf};

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    Prepare {
        #[arg(long)]
        request_file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        node_id: String,
        #[arg(long)]
        central_url: String,
        #[arg(long)]
        config_path: Option<String>,
    },
    Check {
        #[arg(long)]
        context_dir: PathBuf,
        #[arg(long)]
        expected_context: String,
        #[arg(long)]
        work_dir: PathBuf,
        #[arg(long)]
        config_path: Option<String>,
    },
    #[cfg(target_os = "linux")]
    WorkerStatus {
        #[arg(long)]
        work_dir: PathBuf,
        #[arg(long)]
        binding_file: PathBuf,
    },
    #[cfg(target_os = "linux")]
    HostReserve {
        #[arg(long)]
        context_dir: PathBuf,
        #[arg(long)]
        expected_context: String,
        #[arg(long)]
        work_dir: PathBuf,
        #[arg(long)]
        config_path: Option<String>,
    },
    CheckCurrentPolicy {
        #[arg(long)]
        check_work: bool,
        #[arg(long)]
        context_dir: PathBuf,
        #[arg(long)]
        expected_context: String,
        #[arg(long)]
        node_id: String,
        #[arg(long)]
        central_url: String,
        #[arg(long)]
        config_path: Option<String>,
    },
}

pub fn run(args: Args) -> Result<()> {
    match args.command {
        #[cfg(target_os = "linux")]
        Command::WorkerStatus {
            work_dir,
            binding_file,
        } => {
            let binding = crate::native_handoff::worker::read_binding(&binding_file)?;
            ensure!(
                binding.node_id == crate::central_claims::resolve_node_id()?,
                "Native journal belongs to a different worker"
            );
            let run = crate::native_handoff::worker::WorkerRun::open(&work_dir, &binding)?;
            println!("{}", run.inspect()?);
        }
        #[cfg(target_os = "linux")]
        Command::HostReserve {
            context_dir,
            expected_context,
            work_dir,
            config_path,
        } => {
            for key in [
                "GAH_LEDGER_PATH",
                "GAH_EVENTS_PATH",
                "GAH_RECONCILIATION_PATH",
            ] {
                std::env::remove_var(key);
            }
            let cfg = crate::config::load(config_path.as_deref())?;
            let node_id = crate::central_claims::resolve_node_id()?;
            let context = crate::native_handoff::load(
                &cfg,
                &context_dir,
                &expected_context,
                &node_id,
                &work_dir,
            )?;
            println!(
                "{}",
                crate::native_handoff::host::reserve(&context, &work_dir)?
            );
        }
        Command::CheckCurrentPolicy {
            check_work,
            context_dir,
            expected_context,
            node_id,
            central_url,
            config_path,
        } => {
            let cfg = crate::config::load(config_path.as_deref())?;
            let check = if check_work {
                crate::native_handoff::check_current_work
            } else {
                crate::native_handoff::check_current_policy
            };
            let binding = check(
                &cfg,
                &context_dir,
                &expected_context,
                &node_id,
                &central_url,
            )?;
            println!(
                "{}",
                serde_json::json!({"binding":binding,"policy_current":true,"work_current":check_work.then_some(true),"execution_authorized":false})
            );
        }
        Command::Prepare {
            request_file,
            output,
            node_id,
            central_url,
            config_path,
        } => {
            let mut bytes = Vec::new();
            std::fs::File::open(request_file)?
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 8 * 1024 * 1024,
                "Native request exceeds its input limit"
            );
            let request = serde_json::from_slice(&bytes).map_err(|_| {
                anyhow::anyhow!("Native request does not match the supported schema")
            })?;
            let cfg = crate::config::load(config_path.as_deref())?;
            let binding =
                crate::native_handoff::prepare(&cfg, &request, &node_id, &central_url, &output)?;
            println!("{}", serde_json::to_string(&binding)?);
        }
        Command::Check {
            context_dir,
            expected_context,
            work_dir,
            config_path,
        } => {
            // This private child must read the verified job history, not an
            // inherited worker or controller ledger/event path.
            for key in [
                "GAH_LEDGER_PATH",
                "GAH_EVENTS_PATH",
                "GAH_RECONCILIATION_PATH",
            ] {
                std::env::remove_var(key);
            }
            let cfg = crate::config::load(config_path.as_deref())?;
            let node_id = crate::central_claims::resolve_node_id()?;
            let context = crate::native_handoff::load(
                &cfg,
                &context_dir,
                &expected_context,
                &node_id,
                &work_dir,
            )?;
            let mut effective_entries = 0;
            crate::ledger::visit_entries(&context.config, |entry| {
                ensure!(
                    entry.profile == context.binding.profile,
                    "Effective native history has the wrong scope"
                );
                effective_entries += 1;
                Ok(())
            })?;
            ensure!(
                effective_entries == context.history_entries,
                "Effective native history does not match the prepared context"
            );
            println!(
                "{}",
                serde_json::json!({ "binding": context.binding, "history_entries": context.history_entries, "effective_history_entries": effective_entries,
                "execution_started": false, "authority_refresh_required": true })
            );
        }
    }
    Ok(())
}
