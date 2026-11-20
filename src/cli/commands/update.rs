// Command execution for `gah update` (ticket #410).

use anyhow::Result;

use crate::{update as update_module, update::UpdateArgs};

pub use crate::cli::args::UpdateArgs as Args;

pub fn run(args: Args) -> Result<()> {
    let result = update_module::run(UpdateArgs {
        repo: args.repo,
        pull: args.pull,
        agents: args.agent,
        yes: args.yes,
        role: update_module::HostRole::parse(&args.role)?,
        restart_server: args.restart_server,
        server_service: args.server_service,
        from_release: args.from_release,
        release_manifest: args.release_manifest,
        repair_ownership: !args.no_repair_ownership,
    });
    if let Err(error) = &result {
        if error.is::<update_module::WorkerUpdateBusy>() {
            eprintln!("{error}");
            std::process::exit(75); // EX_TEMPFAIL; the worker service re-arms the selected release.
        }
    }
    result
}
