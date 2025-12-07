# 00_EXEC_SUMMARY.md

## Executive Summary: Lighter Exchange Adapter for Nautilus Trader

**Source of truth**: This markdown is the implementation blueprint (Rust core + PyO3/Python layer)
and quick-start/runbook distilled from the latest discovery notes.

**What we're building**: A perpetual futures adapter for Lighter Exchange (zk-rollup DEX) that
follows the standard Nautilus pattern: Rust adapter crate + PyO3 bindings + thin Python wrapper
for configs/factories/tests.

**Scope we can start now**: scaffolding, instrument discovery from public REST, and public market
data with offset tracking. Execution/private streams require validation first.

### Critical Technical Facts

| Aspect | Status |
|--------|--------|
| **Architecture** | Rust-first adapter crate with PyO3 bindings; Python layer only for configs/tests |
| **Auth Model** | Wallet-style signing; exact algorithm + payload hashing **TBD (must validate)** |
| **Auth Token** | Current assumption: token required for private REST/WS/sendTx, but some notes claimed otherwise |
| **Base URLs** | Mainnet: `https://mainnet.zklighter.elliot.ai/` / Testnet: `https://testnet.zklighter.elliot.ai/` |
| **WebSocket** | `wss://mainnet.zklighter.elliot.ai/stream` (channel naming/schema still needs confirmation) |
| **Market Data** | Public order books/trades available; snapshot vs delta semantics unverified |
| **Fees** | Conflicting sources (premium maker/taker 0.002%/0.02% vs 0.02%/0.2%); must verify |

### MUST VALIDATE before execution/private streams

1. **Signing algorithm + tx serialization** — ECDSA vs EdDSA, hashing, encoding, and how nonces are
   bound to the payload.
2. **Auth token necessity** — Whether `/api/v1/account`, private WS, and `sendTx` require a token or
   only signature; discovery notes conflict and must be resolved.
3. **WS schemas** — Channel names and payloads (e.g., `order_book/0` vs `order_book:0`), and whether
   snapshots are ever emitted on subscribe.
4. **Order book semantics** — Confirm REST snapshot + WS delta behavior and offset gap handling rules.
5. **Instrument mapping** — Validate perp metadata fields and precision rules from `orderBooks` before
   locking in `CryptoPerpetual` construction.

### Recommended Next Steps

1. Kick off **PR0 scaffolding** (Rust crate, PyO3 bindings, Python configs/constants, CI skeleton).
2. Implement **PR1 instrument provider** using `orderBooks` (public REST) with fixture-backed tests.
3. Build **public WS market data** with offset tracking; capture real snapshots/deltas into fixtures.
4. Run a **short validation spike on testnet** to answer the MUST VALIDATE items above before coding
   execution/private flows.
5. Proceed to **execution + account** only after signing/auth/WS schemas are proven with captured
   traffic.

**Effort target**: ~4–6 weeks to production-ready v1 with a single senior, assuming validation
questions are closed early.

---
