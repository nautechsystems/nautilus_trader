# LP hedger production deploy config

This directory is the deploy root for the dedicated LP hedger stack.

## Layout

- `lp.live.toml`: shared hidden-backend API config for the LP control surface.
- `lp_stack.env.example`: local smoke environment template for `ops/scripts/deploy/lp_stack.sh`.
- `hedgers/`: one INI per hedger instance, named by exact chainsaw hedger config filename.
- Active checked-in hedgers:
  - `eth_plume_lp_hedger.ini`
  - `eth_plume_lp_hedger_band2.ini`
- Staged checked-in hedgers:
  - `hype_usdt_lp_hedger.ini`
  - `plume_weth_lp_hedger.ini`
- Hidden/template hedgers:
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
- Expose four public `/lp` instances: Band1, Band2, `hype_usdt_lp`, and `plume_weth_lp`.
- Keep `third_lp` hidden until it has a real identity and config.
- Auto-start only Band1 and Band2. `service-hedger3` and `service-hedger4` are staged, Pulse-managed, and stopped by default until their configs are ready.
- Use `docs/runbooks/lp-hedger-production-rollout.md` as the production rollout source of truth for preflight, restart order, smoke validation, and rollback.

## Production control plane

Install the systemd units and seeded env files:

```bash
pnpm --dir fluxboard build
pnpm --dir pulse-ui build
sudo ops/scripts/deploy/install_lp_systemd.sh
sudoedit /etc/flux/common.env
sudoedit /etc/flux/lp-system.ini
sudo chown root:ubuntu /etc/flux/lp-system.ini
sudo chmod 0640 /etc/flux/lp-system.ini
sudo systemctl daemon-reload
sudo systemctl restart flux@tokenmm-api.service
sudo systemctl start flux-lp.target
```

Restart `flux@tokenmm-api.service` after updating `/etc/flux/common.env` so `LP_API_BACKEND_URL` is reloaded before operators expect `/lp` and `/api/v1/hedgers/*` to proxy correctly.

Installer behavior:

- installs `flux@.service`
- installs `/etc/flux/common.env` from `deploy/lp/systemd/common.env.example` if it does not already exist
- installs `/etc/sudoers.d/flux-pulse` for the LP Pulse-managed service set
- writes `/etc/flux/lp-api.env`
- writes `/etc/flux/service-eth-plume-lp-hedger.env`
- writes `/etc/flux/service-eth-plume-lp-hedger-band2.env`
- writes `/etc/flux/service-hedger3.env`
- writes `/etc/flux/service-hedger4.env`
- rewrites `/etc/systemd/system/flux-lp.target` so the target auto-starts `lp-api`, Band1, and Band2 only
- installs `/etc/sudoers.d/flux-pulse` so Pulse can manage `lp-api`, Band1, Band2, `service-hedger3`, and `service-hedger4`

Band1 and Band2 are the live auto-started production pair for this rollout. `hype_usdt_lp` and `plume_weth_lp` are staged checked-in configs: visible on `/lp`, editable through the shared Hedger surface, and enrolled in Pulse/systemd without joining `flux-lp.target`.

Run the host preflight before starting `flux-lp.target`:

```bash
python3 ops/scripts/lp_hedger_preflight.py --json
```

`LP_SYSTEM_CONFIG=/etc/flux/lp-system.ini` is the operator-managed system INI for shared LP runtime settings. It must remain readable by the Flux service user `ubuntu` (for example `root:ubuntu` with mode `0640`). It should provide the same chainsaw sections:

- `[redis]`
- `[plume]`
- `[bybit]`
- `[bybit_hedger]`
- `[bybit_hedger_band2]`

Primary operator surfaces:

- `http://<host>:5022/lp`
- `GET /api/v1/hedgers/instances`
- `GET /api/v1/hedgers/eth_plume_lp`
- `GET /api/v1/hedgers/hype_usdt_lp`
- `GET /api/v1/hedgers/plume_weth_lp`
- `POST /api/v1/hedgers/<hedger_id>/job`
- `http://<host>:5022/pulse`

Post-cutover rollout check:

```bash
bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://127.0.0.1:5022
```

Capture the go/no-go decision, restart times, smoke evidence, residual risks, and rollback note in `docs/reviews/2026-03-09-lp-hedger-prod-rollout.md`.

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
- `/api/v1/hedgers/instances` returns the public `/lp` selector list: `eth_plume_lp`, `eth_plume_lp_band2`, `hype_usdt_lp`, and `plume_weth_lp`.
- `/api/v1/hedgers/hype_usdt_lp` and `/api/v1/hedgers/plume_weth_lp` report staged readiness fields (`staged`, `config_ready`, `config_readiness_errors`) until operators finish their configs.
- `/api/v1/hedgers/eth_plume_lp` returns a chainsaw-compatible hedger payload.
- Pulse shows `service-hedger3` and `service-hedger4` as managed LP jobs, but they remain stopped after install/reboot unless an operator starts them explicitly.

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
- Public selector contract:
  - `eth_plume_lp`
  - `eth_plume_lp_band2`
  - `hype_usdt_lp`
  - `plume_weth_lp`
- `third_lp` remains hidden and template-only as `third_lp_hedger.ini.disabled`.
- Band1 and Band2 are the only auto-started hedgers in this repo snapshot.
