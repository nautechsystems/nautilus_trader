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

   Use a dedicated Redis DB (recommended) to avoid sharing with existing Chainsaw services:

   ```bash
   POC_REDIS_DB=1 python examples/live/poc/makerv3_single_leg_node.py
   ```

3. Run the Nautilus<->Redis bridge.

   ```bash
   python examples/live/poc/chainsaw_bridge.py
   ```

   Match the node Redis DB (or set explicitly):

   ```bash
   python examples/live/poc/chainsaw_bridge.py --redis-db 1 --strategy-id bybit_binance_plumeusdt_makerv3
   ```

4. Run the minimal TokenMM API + UI.

   ```bash
   PORT=5022 python examples/live/poc/nautilus_fluxapi.py
   ```

   Match the same Redis DB as above:

   ```bash
   POC_REDIS_DB=1 PORT=5022 python examples/live/poc/nautilus_fluxapi.py
   ```

5. Open `/tokenmm` pages:

   - `/tokenmm/signal`
   - `/tokenmm/params`
   - `/tokenmm/balances`
   - `/tokenmm/trades`
   - `/tokenmm/alerts`
