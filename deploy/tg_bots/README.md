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
sudo ops/scripts/deploy/install_tg_bots_systemd.sh
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
  Set `telegram_chat_id` and optional `telegram_thread_id` here.

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
- Pulse lists only services whose env files set `PULSE_ENABLED=1`.
- The TG bot group renders under `TG Bots` at `http://<host>:5022/pulse`.

Runbook:

- `docs/runbooks/lan-rogue-trader-alert.md`
