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

## One-file place-and-cancel snippet (mainnet)

```python
import asyncio, os, time, json
from lighter import SignerClient
from lighter.api_client import ApiClient
from lighter.configuration import Configuration
from lighter.api import OrderApi

URL = "https://mainnet.zklighter.elliot.ai"
ACCOUNT_INDEX = int(os.environ["LIGHTER_ACCOUNT_INDEX"])
API_KEY_INDEX = int(os.environ["LIGHTER_API_KEY_INDEX"])
API_KEY_PRIVATE = os.environ["LIGHTER_API_SECRET"].removeprefix("0x")
MARKET_INDEX = 1  # BTC perp

async def main():
    api = ApiClient(Configuration(host=URL))
    ob = (await OrderApi(api).order_book_details(market_id=MARKET_INDEX)).order_book_details[0]
    price = float(ob.last_trade_price)
    price_int = int(round(price * 0.9 * (10 ** ob.price_decimals)))
    size_int = int(round((50.0 / (price * 0.9)) * (10 ** ob.size_decimals)))
    size_int = max(size_int, 1)
    coi = int(time.time_ns() // 1_000_000)

    signer = SignerClient(URL, account_index=ACCOUNT_INDEX, api_private_keys={API_KEY_INDEX: API_KEY_PRIVATE})
    _, create_resp, create_err = await signer.create_order(
        market_index=MARKET_INDEX,
        client_order_index=coi,
        base_amount=size_int,
        price=price_int,
        is_ask=False,
        order_type=SignerClient.ORDER_TYPE_LIMIT,
        time_in_force=SignerClient.ORDER_TIME_IN_FORCE_GOOD_TILL_TIME,
    )
    print("create:", create_resp, create_err)
    await asyncio.sleep(5)
    _, cancel_resp, cancel_err = await signer.cancel_order(market_index=MARKET_INDEX, order_index=coi)
    print("cancel:", cancel_resp, cancel_err)
    await signer.close(); await api.close()

asyncio.run(main())
```

## Open items

- Capture private REST (`account`, `accountActiveOrders`, `nextNonce`) responses with auth token for fixtures.
- Add token refresh logic (deadline-based) and retry on auth failures.
- Confirm private WS auth flow (token in subscribe payload vs header) and message schemas.
