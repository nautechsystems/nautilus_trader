# Blockchain Intelligence & On-Chain Data Landscape

## Entity Attribution & Labeling

Companies that identify who owns which wallet addresses and cluster them into
real-world entities (exchanges, funds, whales, hackers, etc.).

| Company | Coverage | Entity Labels | Free Tier | Specialty |
|---|---|---|---|---|
| **Arkham** | 15 chains (ETH, SOL, BTC, Tron, HyperCore, ...) | Yes (ULTRA ML engine) | Explorer free, API paid | Best retail-facing UI, dedicated HyperCore perp/spot support |
| **Nansen** | 20+ chains | Yes ("Smart Money" labels) | No | Institutional analytics, NFT tracking, fund flow analysis |
| **Chainalysis** | 100+ chains | Yes (Reactor platform) | No | Law enforcement (FBI, IRS, Europol), compliance. $100K+/yr |
| **Elliptic** | 40+ chains | Yes (Lens) | No | AML compliance, bank/fintech integration |
| **TRM Labs** | 25+ chains | Yes | No | Sanctions screening, compliance for exchanges |
| **Crystal Blockchain** | 30+ chains | Yes | No | Compliance/AML, owned by Bitfury |
| **Breadcrumbs** | EVM + BTC | Yes | Free tier | Open-source on-chain investigation tool |
| **Bubblemaps** | EVM | Cluster visualization | Free | Visual token holder clustering, supply analysis |
| **0xScope** | EVM | Yes ("Watchers") | Free tier | Web3 knowledge graph, address profiling |
| **Metasleuth (BlockSec)** | EVM | Yes | Free | Security-focused, hack tracing, fund flow visualization |

### How Entity Labeling Works

These companies identify wallet owners through:

1. **CEX deposit/withdrawal correlation** — timing + amount matching links
   addresses across chains through exchange flows
2. **Cross-chain bridge tracking** — bridge contracts publicly link source
   and destination addresses (Wormhole, LayerZero, Stargate, etc.)
3. **Behavioral fingerprinting** — same transaction timing patterns, gas
   price habits, DeFi interaction sequences across addresses
4. **Public disclosures** — project treasuries, VC portfolio pages, OFAC
   sanctions lists, official announcements
5. **Smart contract deployer analysis** — same key deploying on multiple
   EVM chains = same entity
6. **Funding source analysis** — tracing the origin of funds backward
   through the transaction graph
7. **ENS/domain links** — on-chain name registrations linking to social profiles

Confidence is scored probabilistically:
- **Verified (≥98%)** — multiple independent signals converge
- **Predicted (≥80%)** — single strong signal (e.g., bridge correlation)

### Key Differences

- **Chainalysis** is the industry standard for law enforcement and compliance
  (100+ chains, decades of data). Enterprise-only, no public explorer.
- **Nansen** pioneered "Smart Money" labels for DeFi/NFT. Strongest on
  Ethereum DeFi fund tracking.
- **Arkham** democratized intelligence with a free explorer. Only provider
  with dedicated HyperCore (Hyperliquid perp/spot) API endpoints.
- **Elliptic/TRM/Crystal** focus on compliance/AML — used by exchanges
  to screen deposits and flag sanctioned addresses.

---

## On-Chain Data Platforms (Raw, Queryable)

Platforms that index raw blockchain data and let you query it with SQL or APIs.
No entity attribution — just decoded transactions, events, and state.

| Company | What It Does | Free Tier | API? |
|---|---|---|---|
| **Dune Analytics** | SQL on decoded on-chain data, community dashboards | Yes | Yes |
| **Flipside Crypto** | Similar to Dune, pays analysts for queries | Yes | Yes |
| **Allium** | Enterprise on-chain data warehouse | No | Yes |
| **Transpose** | Real-time SQL on raw chain data | Yes | Yes |
| **Covalent (GoldRush)** | Unified REST API for all EVM chain data | Yes | Yes |
| **Moralis** | Web3 API (balances, transfers, NFTs, DeFi positions) | Yes | Yes |
| **Alchemy** | Node infrastructure + enhanced APIs (transfers, tokens) | Yes | Yes |
| **The Graph** | Decentralized indexing protocol (subgraphs) | Yes | Yes (GraphQL) |

---

## Block Explorers

Per-chain transaction explorers. Free, but single-chain only.

| Explorer | Chain | Free API? |
|---|---|---|
| **Etherscan** | Ethereum | Yes (5 req/s) |
| **Arbiscan** | Arbitrum | Yes |
| **Basescan** | Base | Yes |
| **Polygonscan** | Polygon | Yes |
| **BscScan** | BSC | Yes |
| **Solscan** | Solana | Yes |
| **Hypurrscan** | Hyperliquid (HyperCore + HyperEVM) | No official API |
| **Blockchain.com** | Bitcoin | Yes |

All *scan explorers are operated by the same company (Etherscan) and share a
common API format. Free tier is 5 requests/second per chain.

---

## Portfolio Trackers (Multi-Chain)

Aggregate wallet balances across chains into a single view.

| Company | Free? | API? | Chains |
|---|---|---|---|
| **DeBank** | Yes | Limited free tier | 50+ EVM chains |
| **Zerion** | Yes | Yes | EVM + Solana |
| **Zapper** | Yes | Deprecated public API | EVM |

---

## What We Use (Nautilus Participant Discovery)

| Source | Data | Cost | Notes |
|---|---|---|---|
| **Hyperliquid WS trade stream** | Real-time participant addresses from live trades | Free | Our primary discovery method. 809 instrument subscriptions, ~4000 unique addresses per session |
| **Hyperliquid REST API** | Participant profiles (positions, balances, fills) | Free (1200 weight/min) | Rate-limited. Profile refresh via `WeightedLimiter` |
| **Hypurrscan** | Historical participant data, address lookup | Free | No official API, web scraping fragile |
| **Arkham API** | Entity labels, cross-chain intelligence, HyperCore data | Paid | Only provider with dedicated HyperCore endpoints. Could enrich our discovered addresses with entity attribution |

### Potential Integration Strategy

1. **Discovery** — our WS trade stream (free, real-time, unlimited)
2. **Profiling** — Hyperliquid REST API (free, rate-limited)
3. **Enrichment** — Arkham API for entity labels on high-value addresses (paid)
4. **Historical backfill** — Dune Analytics SQL queries (free tier)