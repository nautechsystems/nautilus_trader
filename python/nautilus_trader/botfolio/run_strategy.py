"""
Trading Engine entry point for bot-folio.

Fetches strategy code from Redis and executes it using Nautilus Trader.
"""
import os
import sys
import time

import redis


def main():
    # 1. Configuration
    redis_url = os.environ.get("REDIS_URL", "redis://localhost:6379")
    bot_id = os.environ.get("BOT_ID")

    if not bot_id:
        print("[TradingEngine] Error: BOT_ID env var not set")
        sys.exit(1)

    print(f"[TradingEngine] Starting strategy for bot {bot_id}...")

    # 2. Fetch Strategy Code
    try:
        r = redis.from_url(redis_url, decode_responses=True)
        code_key = f"bot:{bot_id}:code"
        print(f"[TradingEngine] Fetching code from {code_key}...")

        # Retry loop for fetching code (in case Redis is slow to sync)
        strategy_code = None
        for attempt in range(10):
            strategy_code = r.get(code_key)
            if strategy_code:
                break
            print(f"[TradingEngine] Waiting for code... ({attempt})")
            time.sleep(1)

        if not strategy_code:
            print(f"[TradingEngine] Error: No code found for bot {bot_id}")
            sys.exit(1)

    except Exception as e:
        print(f"[TradingEngine] Error: Redis connection failed: {e}")
        sys.exit(1)

    # 3. Write to disk
    script_path = "/tmp/strategy.py"
    with open(script_path, "w") as f:
        f.write(strategy_code)

    print(f"[TradingEngine] Code written to {script_path}")

    # 4. Execute Strategy
    print("[TradingEngine] Executing strategy...")
    sys.stdout.flush()

    # Replace current process with the python process running the strategy
    os.execl(sys.executable, sys.executable, script_path)


if __name__ == "__main__":
    main()
