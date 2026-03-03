# MakerV3 Single-Leg POC Scaffold

This directory defines the shared translation contract for the MakerV3 single-leg POC:

- `strategy_id`: `bybit_binance_plumeusdt_makerv3`
- Execution venue: Bybit linear perp only
- Market data venues: Bybit + Binance (Binance is data-only)

## Run Order

1. Start Redis.

   ```bash
   redis-server
   ```

2. Run the Nautilus strategy node.

   ```bash
   python examples/live/poc/makerv3_single_leg_node.py
   ```

3. Run the Redis bridge.

   ```bash
   python examples/live/poc/redis_bridge.py
   ```

4. Run the standalone Nautilus GUI/API server.

   ```bash
   PORT=5022 python examples/live/poc/nautilus_fluxapi.py
   ```

5. Open `http://<host>:5022/tokenmm` (or any `/tokenmm/*` route).
