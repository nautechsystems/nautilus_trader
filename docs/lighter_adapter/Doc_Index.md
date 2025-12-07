# Lighter Exchange Adapter Engineering Discovery Pack for Nautilus Trader

## Document Index

This engineering discovery pack contains all documentation required to implement a Lighter Exchange perpetual DEX adapter for the Nautilus Trader framework. The markdown files are the blueprint (Rust core + PyO3) and day-to-day runbook/checklist. All documents are ready to drop into `/docs/lighter_adapter/`.

## Reading Order

1. **00_EXEC_SUMMARY.md** — 5-minute overview and key risks
2. **01_PRD_Lighter_Adapter.md** — Full requirements and scope
3. **03_LIGHTER_API_SPEC.md** — API reference for implementation
4. **02_TECHNICAL_DISCOVERY_Nautilus.md** — Nautilus patterns to follow
5. **04_INTEGRATION_DESIGN.md** — Mapping and event flows
6. **05_IMPLEMENTATION_PLAN.md** — PR-by-PR tasks
7. **06_TESTING_PLAN.md** — Test strategy
8. **07_WORK_BREAKDOWN_TRACKER.md** — Task tracking

## Top 10 Unknowns to Resolve First

| Priority | Unknown | Resolution Method |
|----------|---------|-------------------|
| 1 | **Signing algorithm + tx serialization** | Capture real `sendTx` requests on testnet; confirm curve/hash/encoding with successful submission |
| 2 | **Auth token necessity** | Attempt private REST + WS + `sendTx` with/without token; align on requirement based on captures |
| 3 | **WS channel naming + schema** | Subscribe on testnet and record payloads (`order_book/0` vs `order_book:0`, field naming) |
| 4 | **Snapshot/delta semantics** | Validate whether WS ever sends snapshots, and the exact offset/gap recovery rules |
| 5 | **Instrument mapping correctness** | Verify `orderBooks` fields map cleanly to `CryptoPerpetual` (price/size decimals, min sizes) |
| 6 | **Fee schedule (standard vs premium)** | Confirm maker/taker rates from live responses or support; avoid hardcoding until verified |
| 7 | **Rate limit behavior** | Benchmark Standard vs Premium throttling; set client defaults accordingly |
| 8 | **Nonce fetch/update semantics** | Exercise `nextNonce` (or equivalent) across multiple submissions and restarts |
| 9 | **Error code taxonomy** | Build catalog from live failures; reconcile with any official docs/support replies |
| 10 | **Funding timing/precision** | Observe funding events vs documented schedule to ensure correct accrual + reporting |

## Key Source Links

| Resource | URL |
|----------|-----|
| Nautilus Trader Repo | https://github.com/nautechsystems/nautilus_trader |
| Nautilus Adapters Guide | https://nautilustrader.io/docs/nightly/developer_guide/adapters/ |
| Lighter API Docs | https://apidocs.lighter.xyz/docs/get-started-for-programmers-1 |
| Lighter API Reference | https://apidocs.lighter.xyz/reference/account-1 |
| Lighter Rate Limits | https://apidocs.lighter.xyz/docs/rate-limits |
| Lighter WebSocket Docs | https://apidocs.lighter.xyz/docs/websocket-reference |
| Lighter Python SDK | https://github.com/elliottech/lighter-python |
| Lighter Testnet App | https://testnet.app.lighter.xyz/ |
| dYdX Adapter (Reference) | nautilus_trader/adapters/dydx/ |
| Hyperliquid Adapter (Reference) | nautilus_trader/adapters/hyperliquid/ |
