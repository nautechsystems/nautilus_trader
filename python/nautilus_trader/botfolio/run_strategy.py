"""
Trading Engine entry point for bot-folio.

Fetches strategy code and config from Redis, sets up credentials as environment
variables, then executes the strategy using Nautilus Trader.
"""
import json
import os
import sys
import time

import redis


def fetch_from_redis(r: redis.Redis, key: str, max_attempts: int = 10) -> str | None:
    """Fetch a value from Redis with retry logic."""
    for attempt in range(max_attempts):
        value = r.get(key)
        if value:
            return value
        print(f"[TradingEngine] Waiting for {key}... (attempt {attempt + 1})")
        time.sleep(1)
    return None


def setup_credentials_env(config: dict) -> None:
    """
    Set up environment variables from the config credentials.
    This allows the strategy code to access credentials via standard env vars.
    """
    credentials = config.get("credentials", {})

    # Provider (e.g., 'alpaca')
    provider = credentials.get("provider", "")
    os.environ["BOTFOLIO_PROVIDER"] = provider

    # Trading mode (paper/live)
    trading_mode = credentials.get("tradingMode", "paper")
    os.environ["BOTFOLIO_TRADING_MODE"] = trading_mode

    # Alpaca-specific credentials
    if provider == "alpaca":
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
    # 1. Configuration
    redis_url = os.environ.get("REDIS_URL", "redis://localhost:6379")
    bot_id = os.environ.get("BOT_ID")
    deploy_secret = os.environ.get("DEPLOY_SECRET")

    if not bot_id:
        print("[TradingEngine] Error: BOT_ID env var not set")
        sys.exit(1)

    if not deploy_secret:
        print("[TradingEngine] Error: DEPLOY_SECRET env var not set")
        sys.exit(1)

    print(f"[TradingEngine] Starting strategy for bot {bot_id}...")
    os.environ["BOTFOLIO_BOT_ID"] = bot_id

    # 2. Connect to Redis
    try:
        r = redis.from_url(redis_url, decode_responses=True)
    except Exception as e:
        print(f"[TradingEngine] Error: Redis connection failed: {e}")
        sys.exit(1)

    # 3. Fetch Strategy Code
    # The deploy_secret in the key prevents malicious strategy code from reading
    # other users' configs by scanning Redis keys like `bot:*:config`
    code_key = f"bot:{bot_id}:{deploy_secret}:code"
    print(f"[TradingEngine] Fetching code from Redis...")
    strategy_code = fetch_from_redis(r, code_key)

    if not strategy_code:
        print(f"[TradingEngine] Error: No code found for bot {bot_id}")
        sys.exit(1)

    # 4. Fetch Config (credentials, capital settings)
    config_key = f"bot:{bot_id}:{deploy_secret}:config"
    print(f"[TradingEngine] Fetching config from Redis...")
    config_json = fetch_from_redis(r, config_key)

    if config_json:
        try:
            config = json.loads(config_json)
            setup_credentials_env(config)
            print("[TradingEngine] Credentials and config loaded")
        except json.JSONDecodeError as e:
            print(f"[TradingEngine] Warning: Failed to parse config JSON: {e}")
    else:
        print("[TradingEngine] Warning: No config found, running without credentials")

    # 5. Write strategy to disk
    script_path = "/tmp/strategy.py"
    with open(script_path, "w") as f:
        f.write(strategy_code)

    print(f"[TradingEngine] Code written to {script_path}")
    print("[TradingEngine] Executing strategy...")
    sys.stdout.flush()

    # 6. Execute strategy - replace current process
    os.execl(sys.executable, sys.executable, script_path)


if __name__ == "__main__":
    main()



