# TG Bots production deploy config

This directory is the production deployment root for Telegram bots enrolled in Flux Pulse.

## Layout

- `lan_rogue_trader_alert.ini`: sanitized config template for Lan's rogue trader alert bot.
- `systemd/common.env.example`: shared `flux@.service` environment template when `/etc/flux/common.env` is missing.
- `systemd/flux-tg-bots.target`: target that groups enrolled Telegram bot services.

## Intent

- Supported production lifecycle: install with systemd, then manage the bot from Pulse.
- Checked-in configs stay sanitized. Live secrets do not belong in git.
- The Lan bot uses dedicated env var names so it does not inherit unrelated shared Binance credentials from `/etc/flux/common.env`.
- The Lan bot watches the combined Binance PM + spot balance for the configured asset using the same Binance API key/secret for both requests.
- A missing spot asset row is treated as zero spot balance; HTTP or payload errors still fail the poll.
- The bot also listens for Telegram `/reset` commands from the configured chat and optional topic, then immediately re-baselines the persisted state from the live combined balance.

Dedicated Lan bot environment variables:

- `LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY`
- `LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET`
- `LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN`

Optional AWS Secrets Manager backup IDs:

- `/nautilus/tg-bots/lan_rogue_trader_bot/binance`
- `/nautilus/tg-bots/lan_rogue_trader_bot/telegram_bot`

These `_SECRET_ID` values are operator metadata only. The current
`flux@.service` contract does not auto-load AWS Secrets Manager values into the
runtime env; operators still need the live `LAN_ROGUE_TRADER_BOT_*` secret env
vars in `/etc/flux/tg-bot-lan-rogue-trader-alert.env`.

## Production control plane

```bash
export TG_BOTS_DEPLOY_ROOT=/home/ubuntu/releases/prod/tg_bots/current
cd "${TG_BOTS_DEPLOY_ROOT}"
uv sync --all-groups --all-extras
sudo TG_BOTS_DEPLOY_ROOT="${TG_BOTS_DEPLOY_ROOT}" ops/scripts/deploy/install_tg_bots_systemd.sh
sudoedit /etc/flux/tg-bot-lan-rogue-trader-alert.env
sudoedit /etc/flux/tg-bot-lan-rogue-trader-alert.ini
sudo systemctl daemon-reload
sudo systemctl start flux@tg-bot-lan-rogue-trader-alert.service
```

Required live values:

- `LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY`
- `LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET`
- `LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN`
- `/etc/flux/tg-bot-lan-rogue-trader-alert.ini`
  Set `telegram_chat_id`, optional `telegram_thread_id`, and optional
  `binance_spot_base_url` here.

Optional AWS Secrets Manager backup writes:

```bash
aws secretsmanager create-secret \
  --region ap-southeast-1 \
  --name /nautilus/tg-bots/lan_rogue_trader_bot/binance \
  --secret-string '{"LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY":"...","LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET":"..."}'

aws secretsmanager create-secret \
  --region ap-southeast-1 \
  --name /nautilus/tg-bots/lan_rogue_trader_bot/telegram_bot \
  --secret-string '{"LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN":"..."}'
```

Runtime registration is explicit:

- `flux@.service` reads `/etc/flux/common.env` plus `/etc/flux/<service>.env`.
- The installer seeds `/etc/flux/tg-bot-lan-rogue-trader-alert.ini` from the sanitized template if it does not already exist.
- The installer preserves any existing `LAN_ROGUE_TRADER_BOT_*` secret values on rerun instead of blanking them.
- The installer pins the bot command to the selected release-local `.venv/bin/python`.
- The installer writes release-root `WORKDIR=` / `PYTHONPATH=` overrides into `/etc/flux/tg-bot-lan-rogue-trader-alert.env`.
- The installer resolves the deploy root from `TG_BOTS_DEPLOY_ROOT`, then the existing bot env, then `/etc/flux/common.env`.
- The installer rejects mutable git checkouts and worktrees as live deploy roots.
- Pulse lists only services whose env files set `PULSE_ENABLED=1`.
- The TG bot group renders under `TG Bots` at `http://<host>:5022/pulse`.

Runbook:

- `docs/runbooks/lan-rogue-trader-alert.md`
