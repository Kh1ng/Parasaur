# Prebuilt WSL worker prerequisites

The WSL worker installer installs a downloaded `gah` binary and transpiles the
Node server's TypeScript. It nevertheless included `build-essential` in its
apt prerequisite list. A distribution with all runtime prerequisites but no
native compiler therefore requested sudo just to install unnecessary C/C++
build tools.

Removed that package from this prebuilt installation path. Certificate trust,
curl, Git and xz remain required and are still installed when missing. Source
build/bootstrap paths are unchanged. Node/npm provisioning and backend login
requirements are unchanged.

Two executable regressions run the real shell prerequisite block with isolated
command fixtures, stopping before any installation, network or service action:

- All runtime packages present, compiler package absent: no sudo request.
- curl and compiler package absent: apt installs only curl.

Both failed against the original script, which requested `build-essential`,
then passed after the fix under Linux Node in WSL. Shell commands are stubs,
not a real apt operation; this does not prove a fresh WSL install or provider
execution. The frontend CI's existing Linux/Windows helper matrix now runs the
tests; Windows intentionally skips the POSIX PATH fixture.
