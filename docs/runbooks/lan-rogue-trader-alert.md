# Lan Rogue Trader Alert Runbook

This runbook covers the production rollout and recovery path for the
Pulse-managed Lan rogue trader Telegram bot.

Use it with `deploy/tg_bots/README.md` and
`deploy/tg_bots/lan_rogue_trader_alert.ini`.

## Purpose and scope

- Watch the Binance PM `USDT` balance for Lan's account.
- Send a Telegram baseline plus balance-change alerts with the source bot's
  cooldown, summary, and missing-asset behavior.
- Expose the service in Pulse under the `TG Bots` group.

## Required secrets and config

Service env file:

- `/etc/flux/tg-bot-lan-rogue-trader-alert.env`
- Required env vars:
  - `LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY`
  - `LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET`
  - `LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN`

Optional AWS Secrets Manager backups:

- `/nautilus/tg-bots/lan_rogue_trader_bot/binance`
- `/nautilus/tg-bots/lan_rogue_trader_bot/telegram_bot`

Local config file:

- `/etc/flux/tg-bot-lan-rogue-trader-alert.ini`
- Required fields:
  - `[lan_rogue_trader_alert]`
  - `telegram_chat_id`
  - optional `telegram_thread_id`

## Install and start

```bash
sudo ops/scripts/deploy/install_tg_bots_systemd.sh
sudoedit /etc/flux/tg-bot-lan-rogue-trader-alert.env
sudoedit /etc/flux/tg-bot-lan-rogue-trader-alert.ini
sudo systemctl daemon-reload
sudo systemctl enable flux-tg-bots.target
sudo systemctl start flux@tg-bot-lan-rogue-trader-alert.service
```

## Baseline and Pulse verification

Run:

```bash
curl -fsS http://127.0.0.1:5022/api/pulse/jobs \
  | jq '.jobs[] | select(.group_key == "tg-bots") | {name, group_label, status}'
systemctl is-active flux@tg-bot-lan-rogue-trader-alert.service
journalctl -u flux@tg-bot-lan-rogue-trader-alert.service -n 100 --no-pager
```

Expected:

- one job named `tg-bot-lan-rogue-trader-alert`
- `group_label` is `TG Bots`
- service state is `active`
- logs show clean startup and a baseline send when `send_baseline = true`
- Telegram receives the baseline in the configured chat or topic

## Restart and rollback

Restart:

```bash
sudo systemctl restart flux@tg-bot-lan-rogue-trader-alert.service
```

Stop:

```bash
sudo systemctl stop flux@tg-bot-lan-rogue-trader-alert.service
```

Disable the whole group target:

```bash
sudo systemctl disable flux-tg-bots.target
```

Rollback is config-only:

1. restore the previous `/etc/flux/tg-bot-lan-rogue-trader-alert.env`
2. restore the previous `/etc/flux/tg-bot-lan-rogue-trader-alert.ini`
3. `sudo systemctl daemon-reload`
4. `sudo systemctl restart flux@tg-bot-lan-rogue-trader-alert.service`

## Troubleshooting

- Missing Pulse row:
  Confirm `/etc/flux/tg-bot-lan-rogue-trader-alert.env` exists and keeps
  `PULSE_ENABLED=1`, then run
  `sudo /home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh`.
- Service exits immediately:
  check `WORKDIR`, `PYTHONPATH`, `CMD`, and the three `LAN_ROGUE_TRADER_BOT_*`
  secret env vars in the service env file.
- Telegram sends to the wrong place:
  verify `telegram_chat_id`, `telegram_thread_id`, and `strict_thread` in the
  local INI.
- No baseline message:
  verify `send_baseline = true` in the INI and inspect the unit logs for
  Telegram API failures.
