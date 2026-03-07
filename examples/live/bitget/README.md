# Bitget Live Examples

These examples are smoke tests for the current Bitget adapter surface.

## Current examples

- `bitget_data_tester.py`
  - public market-data example
  - loads `BTCUSDT-PERP.BITGET`
  - subscribes to quote ticks, trade ticks, L2 book deltas, bars, and where applicable mark/index/funding updates
- `bitget_exec_tester.py`
  - execution example
  - authenticates the Bitget execution client
  - listens for private account, order, fill, and position updates
  - can be extended to exercise REST order entry/cancel flows in demo

## Environment variables

Public market data does not require credentials.

Private execution streams require:

- `BITGET_API_KEY`
- `BITGET_API_SECRET`
- `BITGET_API_PASSPHRASE`

`bitget_exec_tester.py` defaults to `demo=True`. Use demo credentials, or edit the
example for mainnet.

## Runbook

Public data tester:

```bash
python -m examples.live.bitget.bitget_data_tester
```

Private stream tester:

```bash
python -m examples.live.bitget.bitget_exec_tester
```
