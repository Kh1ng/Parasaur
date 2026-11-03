# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

The owner operates coding agents and a factory across multiple computers and provider accounts. The desktop and shared web interface are the primary surfaces; iPhone access provides remote control, notifications, and Live Activities.

## Product Purpose

Parasaur should finish repository work through implementation, review, repair, validation, and publication. When work cannot advance, the interface must explain the reason, the evidence, and who or what can release it.

## Positioning

A Tauri desktop control surface combines interactive agent work, configurable workspaces and execution environments, and an unattended factory. T3 Code is a reference for useful daily coding workflows. Electron is explicitly excluded.

## Operating Context

The owner uses Windows and operates a fleet that includes another person's machines. Those machines are not always directly accessible. Assignment in a forge is not proof of a live process; state and decisions must be inspectable through the control plane. Sample data must remain visibly distinct from a connected factory.

## Capabilities and Constraints

The requested beta scope includes multiple accounts for one provider, configurable providers, account usage and limits, spend, Git workflows, updates, workspace and environment configuration, enterprise connectivity, and iOS notifications and Live Activities. Existing implementations are evidence to inspect, not proof that their complete flows work.

The shared React UI runs in web browsers and native shells. Desktop uses Tauri; iOS uses the existing native shell. Preserve authenticated mutation boundaries, review evidence, retry limits, and explicit approval gates. Unknown usage or cost stays unknown.

## Evidence on Hand

- Repository implementations, contracts, and tests.
- `docs/CONTROL_SURFACES.md`: dated capability inventory; verify current code before relying on its status claims.
- GitHub issues and PRs: the source of truth for tracked work.
- Local Windows browser tests, a current-source Tauri executable smoke-tested against sample data, and a locally built NSIS installer. These do not establish installation, signed-update acceptance, or deployed factory health.

## Product Principles

- Make the next useful action apparent, particularly for blocked work.
- Keep live state, cached observations, sample data, and unavailable evidence distinguishable.
- Prefer completing existing flows over introducing parallel systems.
- Preserve work and policy across reconnects, restarts, account switches, and updates.
- Validate user workflows through their actual platform boundaries.
