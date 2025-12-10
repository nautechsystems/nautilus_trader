# PR Notes — Private Auth/Signing Validation

## What we validated

- **Auth token**: `SignerClient.create_auth_token_with_expiry(deadline_seconds, api_key_index, account_index)` returns a token for private REST/WS. Pass it as `Authorization: <token>` header *or* `auth=<token>` query param. No HMAC headers.
- **Nonce**: Per API key. Fetch with `TransactionApi.next_nonce(account_index, api_key_index)`. Retry on `"invalid nonce"` by refreshing the nonce.
- **sendTx**: No auth header needed. Body is multipart form with `tx_type` and `tx_info` where `tx_info` already contains the signature from the signer binary.
- **Account/accountActiveOrders/nextNonce**: Require auth token as above. Header and query both work in the SDK (`authorization` header is accepted).

## Working recipe (mainnet)

1. Install SDK in the repo venv: `.venv/bin/python -m pip install git+https://github.com/elliottech/lighter-python.git`
2. Construct signer:

   ```python
   signer = SignerClient(
       url="https://mainnet.zklighter.elliot.ai",
       account_index=<ACCOUNT_INDEX>,
       api_private_keys={<API_KEY_INDEX>: "<API_KEY_PRIVATE_KEY_NO_0x>"},
   )
   ```

3. Get nonce: `TransactionApi(ApiClient(Configuration(host=url))).next_nonce(account_index, api_key_index)`
4. Sign order: `signer.sign_create_order(...)` using venue ints (price_int, base_amount_int).
5. Submit: `TransactionApi.send_tx(tx_type, tx_info)`
6. Cancel: `signer.cancel_order(market_index, order_index=client_order_index)`

Notes:

- Price scale is `10 ** price_decimals`, size scale is `10 ** size_decimals`.
- BTC perp (market_id=1 on mainnet): `price_decimals=1`, `size_decimals=5`, `min_base_amount=0.00020`.

## Live probe (2025-02-05, mainnet)

- Order placed: limit **buy** BTC perp, ~10% below last.
  - last_price ≈ 90,410.8; target_price ≈ 81,369.7 → `price_int=813697`
  - size for ~$50 notional → `base_amount_int=61` (scaled with size_decimals=5)
  - `client_order_index`: milliseconds from `time.time_ns() // 1_000_000`
  - `create_order` response: `code=200`, tx_hash=`73ddaba96ccd6bc149511e2e9da6626292dfafa6d01a54aebb14fcdb59c4e0508c9dce830a7c4879`
- Cancel after ~5s:
  - `cancel_order` response: `code=200`, tx_hash=`570b4f85522d87fee23f2cb6dd6f2c3b6e20e471bdd4780a9aeb1c244a1dc2a80a19c13fc2ce4105`
- Both calls returned `{"ratelimit": "didn't use volume quota"}`.

## One-file place-and-cancel snippet (mainnet, BTC perp)

```python
import asyncio, os, time
from lighter import SignerClient
from lighter.api_client import ApiClient
from lighter.configuration import Configuration
from lighter.api import OrderApi

URL = os.getenv("LIGHTER_HTTP_BASE", "https://mainnet.zklighter.elliot.ai")
ACCOUNT_INDEX = int(os.environ["LIGHTER_ACCOUNT_INDEX"])
API_KEY_INDEX = int(os.environ["LIGHTER_API_KEY_INDEX"])
# Private key is expected without 0x prefix for SignerClient
API_KEY_PRIVATE = os.environ["LIGHTER_API_SECRET"].removeprefix("0x")
MARKET_INDEX = 1  # BTC perp
NOTIONAL_USD = 50.0
DISCOUNT = 0.10  # 10% below best ask


async def main() -> None:
    api = ApiClient(Configuration(host=URL))

    # 1) Get current book/price to anchor the test order
    ob = (await OrderApi(api).order_book_details(market_id=MARKET_INDEX)).order_book_details[0]
    last_price = float(ob.last_trade_price)
    price_decimals = ob.price_decimals
    size_decimals = ob.size_decimals

    limit_price = last_price * (1.0 - DISCOUNT)
    price_int = int(round(limit_price * (10 ** price_decimals)))

    base_amount = NOTIONAL_USD / limit_price
    size_int = int(round(base_amount * (10 ** size_decimals)))
    size_int = max(size_int, 1)

    client_order_index = int(time.time_ns() // 1_000_000)

    signer = SignerClient(
        url=URL,
        account_index=ACCOUNT_INDEX,
        api_private_keys={API_KEY_INDEX: API_KEY_PRIVATE},
    )

    # 2) Place limit buy ~10% below best ask
    _, create_resp, create_err = await signer.create_order(
        market_index=MARKET_INDEX,
        client_order_index=client_order_index,
        base_amount=size_int,
        price=price_int,
        is_ask=False,
        order_type=SignerClient.ORDER_TYPE_LIMIT,
        time_in_force=SignerClient.ORDER_TIME_IN_FORCE_GOOD_TILL_TIME,
    )
    print("create:", create_resp, create_err)

    # 3) Let it settle on-chain, then cancel
    await asyncio.sleep(5)
    _, cancel_resp, cancel_err = await signer.cancel_order(
        market_index=MARKET_INDEX,
        order_index=client_order_index,
    )
    print("cancel:", cancel_resp, cancel_err)

    await signer.close()
    await api.close()


if __name__ == "__main__":
    asyncio.run(main())
```

## Mainnet validation spike runbook (BTC & ETH perps)

> **Important**: This runbook is designed to be executed manually.  
> The adapter implementation should never hardcode these values; they are only for
> a small “place-and-cancel” probe with ~$50 notional per market.

### Preconditions

- `.env` in the repo root populated with:
  - `LIGHTER_HTTP_BASE`, `LIGHTER_WS_BASE`
  - `LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX`
  - Either `LIGHTER_API_SECRET` (API key private key, with or without `0x` prefix)  
    or `LIGHTER_API_KEY_PRIVATE_KEY` (adapter-specific env used via `resolve_api_key_private_key`).
- Python venv and project deps set up (`make install-debug` or equivalent).
- `lighter-python` SDK installed into the venv:

  ```bash
  . .venv/bin/activate
  python -m pip install "git+https://github.com/elliottech/lighter-python.git"
  ```

### Step 1 — Sanity check env and connectivity

1. Load env and print key routing parameters (no secrets):

   ```bash
   export $(grep -v '^#' .env | xargs)
   echo "env=${LIGHTER_ENV:-mainnet} account_index=${LIGHTER_ACCOUNT_INDEX} api_key_index=${LIGHTER_API_KEY_INDEX}"
   echo "http_base=${LIGHTER_HTTP_BASE:-https://mainnet.zklighter.elliot.ai}"
   ```

2. Confirm public REST works:

   ```bash
   curl -s "${LIGHTER_HTTP_BASE:-https://mainnet.zklighter.elliot.ai}/api/v1/orderBooks" | head -c 512
   ```

   You should see JSON with `code: 200` and an `order_books` array.

### Step 2 — BTC perp place-and-cancel (~$50, 10% below best ask)

1. Run the one-file script above (or equivalent) from the repo root:

   ```bash
   . .venv/bin/activate
   python docs/lighter_adapter/tmp_btc_place_cancel.py  # or `python - <<'PY'` using the snippet inline
   ```

2. Expected behaviour:
   - `create:` response has `code: 200`, a non-empty `tx_hash`, and a `"ratelimit": "didn't use volume quota"` note.
   - `cancel:` response likewise has `code: 200` and a `tx_hash`.
   - The effective price used by the script is ~10% below the live last/ask price and respects `price_decimals`.

3. Optional: verify via REST that the order is no longer active:

   ```bash
   # Requires auth token created via SignerClient.create_auth_token_with_expiry(...)
   curl -s -H "Authorization: ${LIGHTER_AUTH_TOKEN}" \
     "${LIGHTER_HTTP_BASE}/api/v1/accountActiveOrders" | jq '.'
   ```

### Step 3 — ETH perp place-and-cancel (~$50, 10% below best ask)

Repeat Step 2 but with `MARKET_INDEX = 0` (ETH perp) and updated decimals:

- From `orderBooks` / `orderBookDetails`:
  - ETH perp: `market_id=0`, `price_decimals=2`, `size_decimals=4`, `min_base_amount=0.0050`.
- Use the same formulas:
  - `limit_price = best_ask * 0.9`
  - `price_int = round(limit_price * 10 ** price_decimals)`
  - `base_amount = 50.0 / limit_price`
  - `size_int = max(1, round(base_amount * 10 ** size_decimals))`

Capture the `create:` and `cancel:` responses as for BTC, plus the resulting `tx_hash` values.

### Step 4 — Data capture for fixtures and PR3 design

For at least one of the BTC/ETH runs, capture:

- Raw HTTP requests/responses for:
  - `GET /api/v1/orderBooks` or `GET /api/v1/orderBookDetails?market_id={id}`
  - `POST /api/v1/sendTx` for create and cancel
  - `GET /api/v1/account` and `GET /api/v1/accountActiveOrders` with auth token
  - `GET /api/v1/nextNonce` (or equivalent) before/after a nonce mismatch, if you deliberately trigger one.
- WS traffic (if private channels are available) around the lifecycle of the test orders.

These should be redacted and stored under `tests/test_data/lighter/{http,ws}/` and referenced from
`03_LIGHTER_API_SPEC.md` for PR3.

## Open items

- Capture private REST (`account`, `accountActiveOrders`, `nextNonce`) responses with auth token for fixtures.
- Add token refresh logic (deadline-based) and retry on auth failures.
- Confirm private WS auth flow (token in subscribe payload vs header) and message schemas.
