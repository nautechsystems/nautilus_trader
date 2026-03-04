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

2. Load API keys from AWS Secrets Manager.

   ```bash
   export POC_AWS_REGION="ap-southeast-1"
   for secret in /nautilus/makerv3/bybit /nautilus/makerv3/binance /nautilus/makerv3/okx; do
     aws secretsmanager get-secret-value --region "${POC_AWS_REGION}" --secret-id "${secret}" --query SecretString --output text \
       | jq -r 'to_entries[] | "export " + .key + "=" + (.value|@sh)' \
       | while IFS= read -r stmt; do
         eval "${stmt}"
       done
   done
   ```

3. Run the Nautilus strategy node.

   ```bash
  POC_REDIS_PORT=6380 python examples/live/poc/makerv3_single_leg_node.py
  ```

   Start with execution disabled by default (`POC_ENABLE_EXEC=0`). Enable live quoting only after your Bybit API keys are configured in your environment (how the Bybit adapter resolves credentials):

   ```bash
   POC_ENABLE_EXEC=1 POC_REDIS_PORT=6380 python examples/live/poc/makerv3_single_leg_node.py
   ```

4. Run the Nautilus<->Fluxboard bridge.

   ```bash
   python examples/live/poc/chainsaw_bridge.py --redis-port 6380
   ```

   Ensure the same Redis db as the node (`POC_REDIS_DB`, default 0):

   ```bash
   python examples/live/poc/chainsaw_bridge.py --redis-db 0 --redis-port 6380 --strategy-id bybit_binance_plumeusdt_makerv3
   ```

5. Run the minimal TokenMM API + UI.

   ```bash
   POC_REDIS_PORT=6380 PORT=5022 python examples/live/poc/nautilus_fluxapi.py
   ```

   Match the same Redis DB as above:

   ```bash
   POC_REDIS_DB=0 POC_REDIS_PORT=6380 PORT=5022 python examples/live/poc/nautilus_fluxapi.py
   ```

6. Open `http://<host>:5022/tokenmm` (single-page home view with Signal/Params/Balances/Trades/Alerts).

## AWS keystore bootstrap (once-off)

Use these secrets names and keep them aligned with adapter env variables:

- `/nautilus/makerv3/bybit`: `BYBIT_API_KEY`, `BYBIT_API_SECRET`
- `/nautilus/makerv3/binance`: `BINANCE_API_KEY`, `BINANCE_API_SECRET`
- `/nautilus/makerv3/okx`: `OKX_API_KEY`, `OKX_API_SECRET`, `OKX_API_PASSPHRASE`

Create/update from terminal:

```bash
aws secretsmanager put-secret-value --region "${POC_AWS_REGION}" --secret-id /nautilus/makerv3/bybit --secret-string '{"BYBIT_API_KEY":"...","BYBIT_API_SECRET":"..."}'
aws secretsmanager put-secret-value --region "${POC_AWS_REGION}" --secret-id /nautilus/makerv3/binance --secret-string '{"BINANCE_API_KEY":"...","BINANCE_API_SECRET":"..."}'
aws secretsmanager put-secret-value --region "${POC_AWS_REGION}" --secret-id /nautilus/makerv3/okx --secret-string '{"OKX_API_KEY":"...","OKX_API_SECRET":"...","OKX_API_PASSPHRASE":"..."}'
```
