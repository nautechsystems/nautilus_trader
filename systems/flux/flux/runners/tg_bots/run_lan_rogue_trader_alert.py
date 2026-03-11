#!/usr/bin/env python3

from __future__ import annotations

import argparse
from collections.abc import Sequence
from pathlib import Path

from flux.tg_bots.lan_rogue_trader_alert import BinanceSpotClient
from flux.tg_bots.lan_rogue_trader_alert import CombinedBalanceClient
from flux.runners.shared.logging import configure_service_logging
from flux.tg_bots.lan_rogue_trader_alert import BinancePmClient
from flux.tg_bots.lan_rogue_trader_alert import JsonStateStore
from flux.tg_bots.lan_rogue_trader_alert import LanRogueTraderAlertService
from flux.tg_bots.lan_rogue_trader_alert import TelegramNotifier
from flux.tg_bots.lan_rogue_trader_alert import build_http_session
from flux.tg_bots.lan_rogue_trader_alert import load_config


SERVICE_LOG_LEVEL_ENV_VAR = "FLUX_TG_BOTS_LOG_LEVEL"


def _parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the Lan rogue trader Telegram alert bot.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--once", action="store_true", help="Run a single poll and exit.")
    parser.add_argument("--log-level", default=None)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _parse_args(argv)
    logger = configure_service_logging(
        cli_level=args.log_level,
        config_level="INFO",
        service_env_var=SERVICE_LOG_LEVEL_ENV_VAR,
        logger_name=__name__,
    )

    try:
        config = load_config(args.config)
    except Exception as exc:
        logger.error("Failed to load lan rogue trader alert config: %s", exc)
        return 1

    session = build_http_session()
    try:
        pm_client = BinancePmClient(
            base_url=config.binance_base_url,
            asset=config.asset,
            api_key=config.binance_api_key,
            api_secret=config.binance_api_secret,
            session=session,
        )
        spot_client = BinanceSpotClient(
            base_url=config.binance_spot_base_url,
            asset=config.asset,
            api_key=config.binance_api_key,
            api_secret=config.binance_api_secret,
            session=session,
        )
        client = CombinedBalanceClient(pm_client=pm_client, spot_client=spot_client)
        notifier = TelegramNotifier(
            bot_token=config.telegram_bot_token,
            chat_id=config.telegram_chat_id,
            thread_id=config.telegram_thread_id,
            strict_thread=config.strict_thread,
            session=session,
        )
        store = JsonStateStore(config.state_path)
        service = LanRogueTraderAlertService(
            config=config,
            binance_client=client,
            telegram=notifier,
            store=store,
        )

        if args.once:
            service.poll_once()
        else:
            service.run_forever()
        return 0
    except KeyboardInterrupt:
        logger.info("Lan rogue trader alert runner stopped by KeyboardInterrupt")
        return 0
    except Exception:
        logger.exception("Lan rogue trader alert runner terminated unexpectedly")
        return 1
    finally:
        close = getattr(session, "close", None)
        if callable(close):
            close()


__all__ = ["main"]


if __name__ == "__main__":
    raise SystemExit(main())
