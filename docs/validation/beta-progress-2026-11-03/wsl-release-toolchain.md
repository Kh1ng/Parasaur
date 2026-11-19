# WSL release toolchain and central recovery candidate

On 2026-11-04 Windows SSH authentication to the known Parasaur host was denied
(`Permission denied (publickey)`). A private central-owner-token card timed out.
No secret remains pending, and no authenticated live diagnostics or host repair
was performed. The public asset-delivery failure remains unresolved.

## Confirmed Windows/WSL installation defect

The initial cold release install in WSL selected
`/mnt/c/Users/cspur/AppData/Local/hermes/node/npm`, whose adjacent `node.exe`
belongs to Windows. The `node` command itself resolved to Linux Node 22.22.3.
Windows npm rejected the Linux runtime's workspace/lockfile layout. This was
not evidence that the committed lockfile was stale: selecting Linux npm from
the same Node installation installed all 653 locked packages successfully.

Commit `d86a59b4` fixes the shared source/release update command path. An ordinary
Unix npm command stays authoritative. If PATH instead selects a wrapper from a
Windows Node installation, the updater resolves Linux Node's actual executable,
uses its sibling npm, and gives lifecycle scripts that Node directory first.
If the Linux pair cannot be resolved, the update fails rather than falling back
to Windows npm. No user-wide PATH or tool installation is changed.

## Verification and installed CLI

All 90 updater library tests passed in a Linux filesystem source copy. An
initial run directly from the Windows checkout passed 83 but failed seven
existing template/Unix-permission tests; a Git-exported LF source tree and Linux
filesystem resolved those environmental failures. New tests prove Windows-wrapper
replacement, lifecycle Node selection, ordinary Unix npm preservation, and
refusal when the Linux runtime is unavailable.

The optimized CLI then executed its actual `update --from-release` command with
Windows npm still first on the inherited PATH, inside an owned disposable Git
checkout and HOME/CARGO/XDG roots. Systemctl was unavailable by fixture, while
sudo and build tools were blocked. It installed the release's dependencies and
CLI/MCP/alias, preserved the checkout manifests, and created no checkout-level
node_modules. Its installed server served dashboard HTML, complete referenced
JS/CSS bodies and byte ranges; the model-browser recovery chunk was present.

The user's WSL CLI was atomically updated to `d86a59b4`, after verifying that
the previous installed bytes still matched our candidate. Its previous binary
is retained at `~/.cache/parasaur-upgrade-backups/gah-before-d86a59b4`.
Installed CLI SHA-256 is
`af6d5b0984bf16731207d97a1ea093a7c3d464b2e01c40aca9251a15c100aaa9`.
Both read-only native installed-CLI checks passed (account metadata and Mistral
preflight). The native desktop remains `c1a55cf0` and was not launched. No worker
service was installed or started.

Strict library Clippy found one allocation-only comparison in the earlier
Mistral guard. Replacing the owned path conversion with a borrowed path retains
the same comparison and makes `clippy --lib -- -D warnings` pass. This later
source cleanup is not part of the immutable `d86a59b4` candidate.

## Concrete recovery handoff

`artifacts/parasaur-central-d86a59b4.zip` contains the optimized Linux CLI/MCP,
current server/web bundle, public source archive, local manifest, SHA256SUMS,
build information, actual-install check results and reproducible check scripts.
ZIP SHA-256 is
`26b4a8175c68761132866093a54b70b7ebea5403a09424dd4eaa80b551fed37a`.
The README includes compatibility checks, read-only service inspection, and the
actual update command using the operator's confirmed checkout. The web build
embeds `d86a59b4`; the binaries require glibc 2.39, so compatibility with the
real host must be established before installation.

This candidate is unsigned, unpublished and not deployed to the live central.
It contains no desktop installer or credentials. Hermes's listener, Parasaur's
route, production roles, jobs, claims and review gates were not changed. Live
asset repair and an authenticated factory lifecycle remain beta requirements.
