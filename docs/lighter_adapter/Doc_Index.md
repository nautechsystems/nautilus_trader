# Lighter Exchange Adapter Engineering Discovery Pack for Nautilus Trader

## Document Index

This engineering discovery pack contains all documentation required to implement a Lighter Exchange perpetual DEX adapter for the Nautilus Trader framework. All documents are ready to drop into `/docs/lighter_adapter/`.

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
| 1 | **Testnet credentials working** | Obtain credentials, test auth flow |
| 2 | **Complete error code list** | Contact Lighter support; catalog through testing |
| 3 | **WebSocket keepalive requirements** | Test connection stability without ping |
| 4 | **Order book snapshot strategy** | Confirm REST-first approach works |
| 5 | **Nonce persistence strategy** | Decide: file-based vs in-memory with fetch |
| 6 | **Standard vs Premium rate limits impact** | Benchmark with Standard account |
| 7 | **Auth token refresh timing** | Test refresh at various intervals |
| 8 | **Offset gap detection reliability** | Artificially induce gaps, verify detection |
| 9 | **sendTxBatch error handling** | Test with intentionally invalid transactions |
| 10 | **Funding payment precision** | Observe actual funding events vs docs |

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