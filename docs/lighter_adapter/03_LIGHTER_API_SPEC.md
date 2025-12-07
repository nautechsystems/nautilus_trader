# 03_LIGHTER_API_SPEC.md

## API Spec (working notes)

This markdown summarizes the actionable interface checklist and flags uncertainties to resolve
during the validation spike.

### REST Endpoints (to validate)

| Endpoint | Scope | Auth | Notes/TBD |
|----------|-------|------|-----------|
| `GET /api/v1/orderBooks` | Public | None | Primary source for instrument metadata (price/size decimals, mins) |
| `GET /api/v1/orderBookDetails` | Public | None | Some docs reference this path; confirm canonical endpoint name |
| `GET /api/v1/candlesticks` | Public | None | Historical bars; optional for v1 |
| `GET /api/v1/marketStats` | Public | None | Mark/index/funding reference data; confirm payload schema |
| `GET /api/v1/account` | Private | Token? | Current assumption: token required; earlier notes marked “no auth” — must test |
| `GET /api/v1/accountActiveOrders` | Private | Token? | Use for reconciliation; confirm filters + pagination |
| `POST /api/v1/sendTx` | Private | Signature + Token? | Signing algorithm/payload hashing **TBD** |
| `POST /api/v1/sendTxBatch` | Private | Signature + Token? | Batch limits + error behavior **TBD** |
| `GET /api/v1/nextNonce` (or equivalent) | Private | Token? | Confirm path/name and behavior after failed tx |

### WebSocket Channels (to validate)

| Channel | Scope | Notes/TBD |
|---------|-------|-----------|
| `order_book/{market_index}` | Public | Channel delimiter uncertain (`/` vs `:`); confirm snapshot vs delta behavior |
| `trade/{market_index}` | Public | Trade tick feed |
| `market_stats/{market_index}` | Public | Mark/index/funding updates; confirm field names |
| `account_all_orders` (or similar) | Private | Order lifecycle events; payload schema + auth requirement TBD |
| `account_positions` (or similar) | Private | Position/funding events; confirm channel name and fields |

### Capture & Fixture Checklist

- Record HTTP and WS payloads for each endpoint/channel above (testnet), redact secrets, and store
  under `tests/test_data/lighter/{http,ws}/`.
- Verify channel naming and message envelopes (e.g., `{"type":"subscribe"}` vs alternative schema).
- Confirm whether WS sends initial snapshots or requires REST bootstrap only.
- Document the exact signing recipe for `sendTx`/`sendTxBatch`, including nonce rules and hash/curve.
