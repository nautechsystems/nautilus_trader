# Deploy Lanes

This runbook defines the production lane model for the shared Flux host.

## Lanes

- `dev`: mutable canonical repo plus approved worktrees
- `pilot`: pinned release lane for live validation of a specific stack
- `prod`: pinned release lane for approved live trading

`dev` is the only place code is edited.

`pilot` and `prod` are the only places live services run.

## Hard Rules

- No live service may point at `~/nautilus-trader`.
- No live service may point at `.worktrees/*`.
- No live service may point at any mutable checkout under `~`.
- Active pilot and prod release roots are immutable once deployed.
- Live start/stop/restart operations go through `systemd` and Pulse, not ad hoc shell commands.

## Canonical Host Paths

- Canonical dev repo: `~/nautilus-trader`
- Canonical worktree root: `~/nautilus-trader/.worktrees`
- Pilot releases: `~/releases/pilot/<stack>/releases/<timestamp>-<sha>` and `~/releases/pilot/<stack>/current`
- Prod releases: `~/releases/prod/<stack>/releases/<timestamp>-<sha>` and `~/releases/prod/<stack>/current`
- Preserved retired clones and host-layout backups: `~/archive/*`

The path prefix may change later, but the immutable release-root contract must not.

## Service Naming

Each lane uses its own service namespace.

Example for equities:

- prod:
  - `equities-api`
  - `equities-portfolio`
  - `equities-bridge`
  - `equities-node-*`
- pilot:
  - `equities-pilot-api`
  - `equities-pilot-portfolio`
  - `equities-pilot-bridge`
  - `equities-pilot-node-*`

Each lane must also have distinct:

- systemd target
- Pulse group
- ports where applicable
- state or data paths where collision is possible

## Operator Contract

`deploy <stack> to pilot` means:

1. materialize a new pinned pilot release from the chosen dev/worktree source
2. repoint only the pilot `current` symlink
3. regenerate only the pilot env files
4. restart only the pilot services

`bounce <stack> pilot` means:

- restart only the pilot services
- do not rebuild or repoint prod

`promote <stack> pilot to prod` means:

1. take the exact tested pilot release
2. publish it into the prod release lane
3. repoint only prod
4. restart only prod

`deploy <stack> to prod` means:

- deploy from a pinned release only
- never from a dev checkout or worktree

## Agent Contract

Agents must follow these rules:

- use `~/nautilus-trader` as the canonical mutable repo unless docs say otherwise
- create new development worktrees only under `~/nautilus-trader/.worktrees`
- treat `~/releases/pilot/*` and `~/releases/prod/*` as immutable deploy roots
- never point live units at `~/nautilus-trader` or `.worktrees/*`
- never hot-edit active pilot or prod release roots
- update docs and runbooks whenever lane names, env contracts, or rollout steps change

## Repo And Worktree Hygiene

- keep one canonical dev repo
- keep one canonical dev repo at `~/nautilus-trader`
- retire `~/nautilus_trader` and other legacy aliases after reviewing uncommitted work
- retire extra top-level clones after reviewing uncommitted work and archive them under `~/archive/`
- keep one approved worktree location under `~/nautilus-trader/.worktrees`
- remove stale worktrees when they are no longer needed
- treat worktree sprawl as an operational problem on shared hosts, not just a developer convenience issue

## Rollout Order

When introducing a new live lane:

1. define service names and Pulse grouping
2. define release-root layout
3. define state and port separation
4. add or update the stack-specific rollout runbook
5. verify rendered env files before restart

## Related Docs

- `docs/runbooks/equities-pilot-rollout.md`
- `deploy/equities/README.md`
- `deploy/tokenmm/README.md`
- `docs/runbooks/ec2-host-baseline.md`
- `docs/runbooks/production-host-disk-recovery.md`
