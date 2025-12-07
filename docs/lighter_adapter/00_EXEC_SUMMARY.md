# 00_EXEC_SUMMARY.md

## Executive Summary: Lighter Exchange Adapter for Nautilus Trader

**What We're Building**: A complete perpetual futures adapter enabling Nautilus Trader to trade on Lighter Exchange—a zk-rollup-based decentralized perpetual DEX with on-chain order book matching and cryptographic settlement verification.

**Scope**: Full integration covering market data (order books, trades, funding rates), execution (limit/market/stop orders), and account management (positions, margin, PnL reconciliation). Initial release targets Python-first implementation with optional Rust core components for performance-critical paths.

**Key Architecture Decision**: Use **dYdX** and **Hyperliquid** adapters as primary reference implementations—both are perp DEX adapters with similar wallet-based authentication patterns and funding rate streams.

### Critical Technical Facts

| Aspect | Lighter Specification |
|--------|----------------------|
| **Auth Model** | API key private key signing (not traditional API key/secret) |
| **Base URLs** | Mainnet: `https://mainnet.zklighter.elliot.ai/` / Testnet: `https://testnet.zklighter.elliot.ai/` |
| **WebSocket** | `wss://mainnet.zklighter.elliot.ai/stream` |
| **Rate Limits** | Standard: 60 req/min; Premium: 24,000 weighted req/min |
| **Order Types** | Limit, Market, Stop Loss, Take Profit, TWAP |
| **Funding** | Hourly, clamped ±0.5% |

### Top 5 Risks

1. **No WS Order Book Snapshot** — Must fetch REST snapshot first, then apply deltas (offset sequencing critical)
2. **Nonce Management Complexity** — Per-API-key nonce tracking required; race conditions possible
3. **Standard Account Rate Limits** — 60 req/min severely limits trading frequency without Premium
4. **Auth Token Expiry** — 8-hour max validity; must implement refresh logic
5. **Missing Error Code Documentation** — Incomplete error taxonomy in official docs

### Recommended Next Steps

1. **Immediate**: Obtain testnet credentials and validate auth flow end-to-end
2. **Week 1**: Implement instrument provider and basic REST client
3. **Week 2**: Build WebSocket client with order book synchronization
4. **Week 3**: Add execution client and order lifecycle handling
5. **Week 4**: Account reconciliation and hardening

**Estimated Effort**: 4-6 weeks for production-ready v1 with single senior developer.

---