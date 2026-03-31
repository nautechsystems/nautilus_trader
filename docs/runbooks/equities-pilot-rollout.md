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
- `equities-pilot` binds its hidden API to `127.0.0.1:5124` and defaults to `EQUITIES_REDIS_DB=1` so pilot state stays out of prod Redis DB `0`
- Live validation and debugging should use the managed equities ElastiCache endpoint from `/etc/flux/common.env` as the source of truth; `redis-cli` is only the client process used to inspect it.
- `EQUITIES_REDIS_DB` applies to the Redis-backed cache, message bus, bridge/API reads, and direct profile/runtime writers, so pilot lane DB overrides must be validated end to end before restart.

## Deploy To Pilot

`deploy equities to pilot` means:

1. choose the source commit or worktree in `dev`
2. materialize a new pilot release under `~/releases/pilot/equities/releases/<timestamp>-<sha>`
3. repoint `~/releases/pilot/equities/current`
4. regenerate `/etc/flux/equities-pilot*.env`
5. verify the rendered env files point only at the pilot release root
6. restart only the `equities-pilot` services

Reference commands:

```bash
export SOURCE_ROOT=~/nautilus_trader/.worktrees/<your-worktree>
export SOURCE_REF="$(git -C "${SOURCE_ROOT}" rev-parse --short HEAD)"
export RELEASE_ROOT="$(DEPLOY_LANE=pilot \
  STACK_NAME=equities \
  SOURCE_ROOT="${SOURCE_ROOT}" \
  SOURCE_REF="${SOURCE_REF}" \
  "${SOURCE_ROOT}/ops/scripts/deploy/create_release_root.sh")"
cd ~/releases/pilot/equities/current
uv sync --all-groups --all-extras
sudo EQUITIES_DEPLOY_ROOT=~/releases/pilot/equities/current \
  EQUITIES_DEPLOY_LANE=pilot \
  ops/scripts/deploy/install_equities_systemd.sh
```

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
- confirm `EQUITIES_REDIS_DB=1` unless you intentionally chose a different pilot Redis DB
- confirm no pilot env points at `~/nautilus_trader` or `.worktrees/*`
- confirm pilot API binds `127.0.0.1:5124`, not the prod backend port `127.0.0.1:5024`
- if pilot and prod will run concurrently on the same Redis host, keep pilot on its own Redis DB unless you intentionally want shared state

After restart:

- inspect `systemctl status 'flux@equities-pilot*' --no-pager`
- inspect Pulse group status at `/api/pulse/jobs`
- inspect the pilot backend directly on `http://127.0.0.1:5124`

## Debugging Workflow

Use this loop:

1. observe issue in `equities-pilot`
2. inspect pilot logs and state
3. fix code in `dev` worktree
4. create a new pilot release
5. bounce pilot
6. re-validate

Do not patch the active pilot release root by hand.
