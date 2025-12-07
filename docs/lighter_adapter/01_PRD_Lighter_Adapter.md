# 01_PRD_Lighter_Adapter.md

## Product Requirements Document: Lighter Exchange Adapter

### Background & Goals

**Background**: Nautilus Trader is a high-performance algorithmic trading platform supporting multiple venues. Lighter Exchange is an emerging zk-rollup-based perpetual DEX offering low fees (0% maker/taker for Standard accounts) and cryptographic trade settlement. Adding Lighter expands Nautilus's DEX coverage alongside existing dYdX and Hyperliquid adapters.

**Goals**:
- Enable Nautilus users to trade perpetual futures on Lighter Exchange
- Maintain Nautilus's high code quality standards (strong typing, deterministic state, testability)
- Support both live trading and backtesting workflows
- Achieve sub-second order round-trip latency on Premium accounts

### Non-Goals (Explicitly Excluded from v1)

- Spot trading (Lighter is perps-only)
- Public pool/LLP participation features
- Sub-account management beyond primary account
- Mobile/desktop app API key indices (0, 1)
- Historical data backfill beyond 30 days
- Cross-exchange arbitrage features

### User Personas & Stories

**Persona 1: Quantitative Trader**
- *As a quant trader, I want to deploy my existing Nautilus strategies on Lighter so I can access DeFi liquidity with minimal code changes*

**Persona 2: Market Maker**
- *As a market maker, I need real-time order book updates and fast order placement to maintain tight spreads on Lighter markets*

**Persona 3: Strategy Developer**
- *As a strategy developer, I want to backtest against historical Lighter data and seamlessly transition to live trading*

### Functional Requirements

#### FR-1: Instrument Discovery
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-1.1 | Load all perpetual markets from `orderBooks` endpoint | P0 |
| FR-1.2 | Parse precision rules (size_decimals, price_decimals) | P0 |
| FR-1.3 | Map to `CryptoPerpetual` instrument type | P0 |
| FR-1.4 | Support filtering by market_index | P1 |

#### FR-2: Market Data
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-2.1 | Subscribe to order book deltas via WebSocket | P0 |
| FR-2.2 | Fetch order book snapshots via REST | P0 |
| FR-2.3 | Maintain synchronized order book with offset tracking | P0 |
| FR-2.4 | Subscribe to trade feed | P0 |
| FR-2.5 | Subscribe to market stats (mark price, index price, funding) | P0 |
| FR-2.6 | Request historical candlesticks via REST | P1 |

#### FR-3: Order Execution
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-3.1 | Submit limit orders | P0 |
| FR-3.2 | Submit market orders | P0 |
| FR-3.3 | Cancel orders by order_index | P0 |
| FR-3.4 | Batch cancel orders | P1 |
| FR-3.5 | Support stop-loss and take-profit orders | P1 |
| FR-3.6 | Map client_order_index ↔ Nautilus ClientOrderId | P0 |
| FR-3.7 | Handle reduce-only orders | P1 |

#### FR-4: Account Management
| ID | Requirement | Priority |
|----|-------------|----------|
| FR-4.1 | Query account balances and collateral | P0 |
| FR-4.2 | Query open positions | P0 |
| FR-4.3 | Track unrealized/realized PnL | P0 |
| FR-4.4 | Subscribe to position updates via WebSocket | P0 |
| FR-4.5 | Query fill history | P1 |
| FR-4.6 | Handle funding payments | P1 |

### Non-Functional Requirements

| Category | Requirement | Target |
|----------|-------------|--------|
| **Latency** | Order submission round-trip | &lt;500ms (Premium) |
| **Reliability** | Auto-reconnect on WS disconnect | Within 5 seconds |
| **Correctness** | Order state consistency | 100% reconciliation on reconnect |
| **Observability** | Structured logging for all events | INFO level default |
| **Testability** | Unit test coverage | &gt;80% for parsers/mappers |

### Acceptance Criteria

- [ ] Can load all Lighter perpetual instruments
- [ ] Order book stays synchronized within 100ms of exchange state
- [ ] Can submit and cancel limit orders successfully
- [ ] Position and balance updates reflect within 1 second of fill
- [ ] Adapter reconnects automatically after network disruption
- [ ] All public API methods have corresponding unit tests
- [ ] Integration tests pass against testnet

### Dependencies

| Dependency | Purpose | Version |
|------------|---------|---------|
| `websockets` | WebSocket client | &gt;=12.0 |
| `aiohttp` | Async HTTP client | &gt;=3.9 |
| `msgspec` | Fast JSON parsing | &gt;=0.18 |
| `lighter-sdk` (optional) | Reference for signing | latest |

### Rollout Plan

| Phase | Milestone | Criteria |
|-------|-----------|----------|
| **Alpha** | Internal testing | Basic order flow works on testnet |
| **Beta** | Limited users | Market data + execution stable for 1 week |
| **GA** | Public release | Full test coverage, docs complete |

---