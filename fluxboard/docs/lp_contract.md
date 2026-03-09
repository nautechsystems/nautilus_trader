# LP Fluxboard Contract

This document freezes the v1 LP operator surface for the monorepo port.

## Public routes

- `/lp`
- `/lp/hedger`
- `/api/v1/hedgers/instances`
- `/api/v1/hedgers/<hedger_id>`
- `/api/v1/hedgers/<hedger_id>/job`
- `/api/v1/hedgers/<hedger_id>/geometry-overrides`
- `/api/v1/hedgers/<hedger_id>/threshold-overrides`
- `/api/v1/hedgers/<hedger_id>/enabled`
- `/api/v1/hedgers/<hedger_id>/events/clear`

## Hidden backend contract

- Hidden backend bind: `127.0.0.1:5025`
- Public proxy env: `LP_API_BACKEND_URL=http://127.0.0.1:5025`
- Public host stays the shared Flux host on `:5022`

## Hedger identity contract

- Preserve chainsaw hedger IDs.
- Preserve chainsaw Pulse job IDs.
- Preserve chainsaw config env var names.
- Preserve chainsaw-compatible payload aliases for ETH/PLUME while using token0/token1 as the internal model.

## Redis key family

Each LP hedger uses the same Redis family keyed by `<state_key>`:

- `<state_key>:state`
- `<state_key>:snapshot`
- `<state_key>:events`
- `<state_key>:mode`
- `<state_key>:geometry_overrides`
- `<state_key>:threshold_overrides`

## Cleanup decisions in this repo

- `/lp` is the canonical Fluxboard route; legacy default-surface `/hedger` is retired.
- Target values remain config-driven; they are not silently recomputed from geometry.
- Band2 no longer clears `REDIS_URL`.
- Checked-in configs preserve key names but scrub live secrets.
- Extra hedgers ship as `.ini.disabled` until validated.
