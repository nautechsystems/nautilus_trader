# Equities Pilot Rollout

This runbook defines the operator workflow for the first formal `equities-pilot` lane.

## Purpose

Use `equities-pilot` when a strategy or stack change needs live validation without mutating equities prod.

Typical use cases:

- live market-data and shadow-logic validation
- bounded live trading validation
- fast restart-and-observe debugging on the shared host

## Lane Contract

- `equities-pilot` runs from a pinned pilot release root
- `equities-pilot` has distinct service IDs from equities prod
- `equities-pilot` appears as its own Pulse group
- `equities-pilot` may be bounced independently of equities prod

## Deploy To Pilot

`deploy equities to pilot` means:

1. choose the source commit or worktree in `dev`
2. materialize a new pilot release under `~/releases/pilot/equities/releases/<timestamp>-<sha>`
3. repoint `~/releases/pilot/equities/current`
4. regenerate `/etc/flux/equities-pilot*.env`
5. verify the rendered env files point only at the pilot release root
6. restart only the `equities-pilot` services

## Bounce Pilot

`bounce equities pilot` means:

- restart only the `equities-pilot` services
- do not change prod env files
- do not change prod release roots

## Promote Pilot To Prod

`promote equities pilot to prod` means:

1. take the exact tested pilot release
2. publish it into the prod release lane
3. repoint prod `current`
4. regenerate prod env files
5. restart only equities prod

## Validation

Before restart:

- inspect `/etc/flux/equities-pilot*.env`
- confirm `WORKDIR` and `PYTHONPATH` point at the pilot release root
- confirm no pilot env points at `~/nautilus_trader` or `.worktrees/*`
- confirm pilot ports and state paths do not collide with prod

After restart:

- inspect `systemctl status 'flux@equities-pilot*' --no-pager`
- inspect Pulse group status at `/api/pulse/jobs`
- inspect the pilot `/equities` surface and pilot-backed API routes as defined by the current deploy contract

## Debugging Workflow

Use this loop:

1. observe issue in `equities-pilot`
2. inspect pilot logs and state
3. fix code in `dev` worktree
4. create a new pilot release
5. bounce pilot
6. re-validate

Do not patch the active pilot release root by hand.
