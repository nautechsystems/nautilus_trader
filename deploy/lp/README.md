# LP hedger production deploy config

This directory is the deploy root for the dedicated LP hedger stack.

## Layout

- `lp.live.toml`: shared hidden-backend API config for the LP control surface.
- `lp_stack.env.example`: local smoke environment template for `ops/scripts/deploy/lp_stack.sh`.
- `hedgers/`: one INI per hedger instance, named by exact chainsaw hedger config filename.
- Active checked-in hedgers:
  - `eth_plume_lp_hedger.ini`
  - `eth_plume_lp_hedger_band2.ini`
- Disabled/template hedgers:
  - `hype_usdt_lp_hedger.ini.disabled`
  - `plume_weth_lp_hedger.ini.disabled`
  - `third_lp_hedger.ini.disabled`
  - `lp_hedger.template.ini`
- Systemd assets:
  - `systemd/common.env.example`
  - `systemd/flux-lp.target`
  - `systemd/flux-pulse.sudoers`

## Intent

- Keep the public LP surface stable at `/lp`.
- Keep the public hedger API stable at `/api/v1/hedgers/*`.
- Run the LP backend as a hidden loopback service at `LP_API_BACKEND_URL=http://127.0.0.1:5025`.
- Preserve the chainsaw job IDs, hedger IDs, Redis key families, config/env key names, and payload field names.
- Keep checked-in configs sanitized: same key names as chainsaw, but no live Bybit secrets in git.
- Auto-enroll only Band1 and Band2. Extra configs stay `.ini.disabled` until pool geometry and credentials are validated.
- Use `docs/runbooks/lp-hedger-production-rollout.md` as the production rollout source of truth for preflight, restart order, smoke validation, and rollback.

## Production control plane

Install the systemd units and seeded env files:

```bash
sudo ops/scripts/deploy/install_lp_systemd.sh
sudoedit /etc/flux/common.env
sudoedit /etc/flux/lp-system.ini
sudo systemctl daemon-reload
sudo systemctl start flux-lp.target
```

Installer behavior:

- installs `flux@.service`
- installs `/etc/flux/common.env` from `deploy/lp/systemd/common.env.example` if it does not already exist
- installs `/etc/sudoers.d/flux-pulse` for the LP Pulse-managed service set
- writes `/etc/flux/lp-api.env`
- writes `/etc/flux/service-eth-plume-lp-hedger.env`
- writes `/etc/flux/service-eth-plume-lp-hedger-band2.env`
- rewrites `/etc/systemd/system/flux-lp.target` so the target enrolls `lp-api`, Band1, and Band2 only

Band1 and Band2 are the only active checked-in production instances for this rollout.

`LP_SYSTEM_CONFIG=/etc/flux/lp-system.ini` is the operator-managed system INI for shared LP runtime settings. It should provide the same chainsaw sections:

- `[redis]`
- `[plume]`
- `[bybit]`
- `[bybit_hedger]`
- `[bybit_hedger_band2]`

Primary operator surfaces:

- `http://<host>:5022/lp`
- `GET /api/v1/hedgers/instances`
- `GET /api/v1/hedgers/eth_plume_lp`
- `POST /api/v1/hedgers/<hedger_id>/job`
- `http://<host>:5022/pulse`

For the full production cutover and rollback procedure, follow `docs/runbooks/lp-hedger-production-rollout.md`.

## Local smoke only

For a local smoke bring-up outside systemd:

```bash
cp deploy/lp/lp_stack.env.example deploy/lp/lp_stack.env
LP_MODE=paper \
LP_CONFIRM_LIVE=0 \
LP_ENABLE_EXECUTION=0 \
ops/scripts/deploy/lp_stack.sh start
```

Smoke-check the public LP surfaces directly:

```bash
curl -fsS http://127.0.0.1:5022/lp
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/instances
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/eth_plume_lp
ops/scripts/deploy/lp_stack.sh stop
```

Expected smoke result:

- `/lp` serves the Fluxboard SPA.
- `/api/v1/hedgers/instances` returns the registered hedger list.
- `/api/v1/hedgers/eth_plume_lp` returns a chainsaw-compatible hedger payload.

## Contracts

- Redis key families stay chainsaw-compatible:
  - `<state_key>:state`
  - `<state_key>:snapshot`
  - `<state_key>:events`
  - `<state_key>:mode`
  - `<state_key>:geometry_overrides`
  - `<state_key>:threshold_overrides`
- Public host proxy contract:
  - `/lp`
  - `/api/v1/hedgers/*`
  - `LP_API_BACKEND_URL=http://127.0.0.1:5025`
- Checked-in rollback/template configs remain `.ini.disabled` and are deliberately not auto-enrolled.
- Band1 and Band2 are the only active checked-in hedgers in this repo snapshot.
