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

## Production rollout contract

- `/lp` remains the operator-facing home for the shared LP surface on the public `:5022` host.
- `/api/v1/hedgers/*` remains the only public LP API family.
- `/api/v1/hedgers/instances` drives the `/lp` selector and must expose only the active production pair (Band1 and Band2) during this rollout.
- `LP_API_BACKEND_URL=http://127.0.0.1:5025` remains the hidden backend contract.
- Built Fluxboard static files must resolve from the neutral shared prefix `/static/fluxboard/*`; `/lp` and `/tokenmm` stay SPA entry routes, not asset owners.
- Band1 and Band2 are the only active checked-in production instances during rollout.
- The shared surface must keep the Chainsaw-visible controls available: instance selector, state pills, restart, enable/disable, config editing, geometry overrides, threshold overrides, and recent-hedges clear.

## Intentional monorepo deltas versus Chainsaw

- non-ETH hedgers use the same generic by-ID operator controls for geometry overrides, threshold overrides, enable/disable, and recent-hedges clear.
- Edit Config remains available for ETH/PLUME Band1 and Band2 on `/lp`, even though Chainsaw limited that control more narrowly; target values still remain config-driven.
