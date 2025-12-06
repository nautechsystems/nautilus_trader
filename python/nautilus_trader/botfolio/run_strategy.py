"""
Trading Engine entry point for bot-folio.

Fetches strategy code and config from Redis, sets up credentials as environment
variables, then executes the strategy using Nautilus Trader.

All output is captured and persisted to Redis before exit so logs are always
available even after the container is removed.
"""
import io
import json
import os
import sys
import time
import traceback
from contextlib import redirect_stdout, redirect_stderr
from datetime import datetime, timezone

import redis


class TeeWriter:
    """Write to multiple streams simultaneously."""
    def __init__(self, *streams):
        self.streams = streams

    def write(self, data):
        for stream in self.streams:
            stream.write(data)
            stream.flush()

    def flush(self):
        for stream in self.streams:
            stream.flush()


# Global log capture buffer
_log_buffer = io.StringIO()
_redis_client: redis.Redis | None = None
_bot_id: str | None = None


def _persist_logs():
    """Persist captured logs to Redis before exit."""
    if not _redis_client or not _bot_id:
        return
    try:
        logs = _log_buffer.getvalue()
        timestamp = datetime.now(timezone.utc).isoformat()
        log_entry = json.dumps({
            "logs": logs,
            "timestamp": timestamp,
            "exitedAt": timestamp
        })
        # Persist to Redis with 24h TTL so logs are available after container dies
        _redis_client.setex(f"bot:{_bot_id}:logs", 86400, log_entry)
    except Exception as e:
        # Last resort - print to original stderr
        sys.__stderr__.write(f"[TradingEngine] Failed to persist logs: {e}\n")


def log(message: str):
    """Log a message with timestamp."""
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
    print(f"{ts} [TradingEngine] {message}")


def fetch_from_redis(r: redis.Redis, key: str, max_attempts: int = 10) -> str | None:
    """Fetch a value from Redis with retry logic."""
    for attempt in range(max_attempts):
        value = r.get(key)
        if value:
            return value
        log(f"Waiting for {key}... (attempt {attempt + 1})")
        time.sleep(1)
    return None


def setup_credentials_env(config: dict) -> None:
    """
    Set up environment variables from the config credentials.
    This allows the strategy code to access credentials via standard env vars.
    """
    credentials = config.get("credentials", {})

    # Provider (e.g., 'alpaca', 'botfolio')
    provider = credentials.get("provider", "")
    os.environ["BOTFOLIO_PROVIDER"] = provider

    # Trading mode (paper/live)
    trading_mode = credentials.get("tradingMode", "paper")
    os.environ["BOTFOLIO_TRADING_MODE"] = trading_mode

    if provider == "botfolio":
        # Local paper trading with bot-folio adapter
        # No external credentials needed - uses Redis for market data
        os.environ["BOTFOLIO_ADAPTER"] = "local"
        os.environ["BOTFOLIO_REDIS_URL"] = os.environ.get("REDIS_URL", "redis://localhost:6379")
        log("Using bot-folio local adapter for paper trading")

    elif provider == "alpaca":
        # Alpaca external broker
        os.environ["BOTFOLIO_ADAPTER"] = "alpaca"

        # API Key authentication
        if "apiKey" in credentials:
            os.environ["APCA_API_KEY_ID"] = credentials["apiKey"]
            os.environ["APCA_API_SECRET_KEY"] = credentials["apiSecret"]
        # OAuth authentication
        elif "accessToken" in credentials:
            os.environ["APCA_API_ACCESS_TOKEN"] = credentials["accessToken"]

        # Set paper trading flag
        is_paper = trading_mode == "paper"
        os.environ["APCA_API_BASE_URL"] = (
            "https://paper-api.alpaca.markets" if is_paper else "https://api.alpaca.markets"
        )

    # Capital settings
    os.environ["BOTFOLIO_INITIAL_CAPITAL"] = str(config.get("initialCapital", 100000))
    os.environ["BOTFOLIO_VIRTUAL_CASH"] = str(config.get("virtualCash", 100000))


def main():
    global _redis_client, _bot_id

    # Set up output capture - write to both console and buffer
    tee_stdout = TeeWriter(sys.__stdout__, _log_buffer)
    tee_stderr = TeeWriter(sys.__stderr__, _log_buffer)
    sys.stdout = tee_stdout
    sys.stderr = tee_stderr

    exit_code = 0
    try:
        # 1. Configuration
        redis_url = os.environ.get("REDIS_URL", "redis://localhost:6379")
        _bot_id = os.environ.get("BOT_ID")
        deploy_secret = os.environ.get("DEPLOY_SECRET")

        if not _bot_id:
            log("Error: BOT_ID env var not set")
            exit_code = 1
            return

        if not deploy_secret:
            log("Error: DEPLOY_SECRET env var not set")
            exit_code = 1
            return

        log(f"Starting strategy for bot {_bot_id}...")
        os.environ["BOTFOLIO_BOT_ID"] = _bot_id

        # 2. Connect to Redis
        try:
            _redis_client = redis.from_url(redis_url, decode_responses=True)
        except Exception as e:
            log(f"Error: Redis connection failed: {e}")
            exit_code = 1
            return

        # 3. Fetch Strategy Code
        code_key = f"bot:{_bot_id}:{deploy_secret}:code"
        log("Fetching code from Redis...")
        strategy_code = fetch_from_redis(_redis_client, code_key)

        if not strategy_code:
            log(f"Error: No code found for bot {_bot_id}")
            exit_code = 1
            return

        # 4. Fetch Config (credentials, capital settings)
        config_key = f"bot:{_bot_id}:{deploy_secret}:config"
        log("Fetching config from Redis...")
        config_json = fetch_from_redis(_redis_client, config_key)

        if config_json:
            try:
                config = json.loads(config_json)
                setup_credentials_env(config)
                log("Credentials and config loaded")
            except json.JSONDecodeError as e:
                log(f"Warning: Failed to parse config JSON: {e}")
        else:
            log("Warning: No config found, running without credentials")

        # 5. Write strategy to disk
        script_path = "/tmp/strategy.py"
        with open(script_path, "w") as f:
            f.write(strategy_code)

        log(f"Code written to {script_path}")
        log("Executing strategy...")
        sys.stdout.flush()

        # 6. Execute strategy using exec() so we can catch errors
        try:
            with open(script_path) as f:
                code = f.read()
            exec(compile(code, script_path, 'exec'), {'__name__': '__main__', '__file__': script_path})
        except Exception as e:
            log(f"FATAL: Strategy execution failed: {type(e).__name__}: {e}")
            traceback.print_exc()
            exit_code = 1

    except Exception as e:
        log(f"FATAL: Unexpected error: {type(e).__name__}: {e}")
        traceback.print_exc()
        exit_code = 1

    finally:
        # Always persist logs before exit
        log(f"Container exiting with code {exit_code}")
        _persist_logs()
        sys.exit(exit_code)


if __name__ == "__main__":
    main()
