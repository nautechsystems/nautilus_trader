# PR Notes — Private Auth/Signing Validation

## What we validated

- **Signer + auth token (SDK-free)**: Loading `lighter-signer-*.{dylib,so,dll}` with `ctypes` works
  without installing the `lighter-python` SDK. Call
  `CreateClient(base_url, api_key_no_0x, chain_id, api_key_index, account_index)` (chain_id 304
  mainnet, 300 testnet), then `CreateAuthToken(deadline_seconds + now, api_key_index, account_index)`
  for a bearer token.
- **Auth scope**: `accountActiveOrders` requires the token (`Authorization` header or `auth` query)
  and returns HTTP 400/code `20001` if missing. `/api/v1/account` currently responds 200 without
  auth; `/api/v1/nextNonce` also works unauthenticated.
- **Nonce**: Per API key via `/api/v1/nextNonce?account_index=<>&api_key_index=<>`; refresh on
  `"invalid nonce"` errors.
- **sendTx**: No auth header needed. Submit `multipart/form-data` with `tx_type`, `tx_info`, and
  optional `price_protection` (defaults `true`). `tx_info` is the signer-produced JSON (Sig
  included).
- **Price/size scales**: `price_int = price * 10 ** price_decimals`, `base_amount_int = size * 10 **
  size_decimals`. Mainnet perps: BTC `price_decimals=1`, `size_decimals=5`, `min_base=0.00020`; ETH
  `price_decimals=2`, `size_decimals=4`, `min_base=0.0050`.

## SDK-free working recipe (mainnet)

1. Make a signer binary available (from `lighter-python/lighter/signers/`): choose the platform file
   (e.g., `lighter-signer-darwin-arm64.dylib` on Apple Silicon). Chain IDs: mainnet `304`, testnet
   `300`.
2. Initialize signer, fetch nonce, sign, and send via raw HTTP:

   ```python
   import ctypes, os, pathlib, platform, time, requests
   from decimal import Decimal

   HTTP_BASE = os.getenv("LIGHTER_HTTP_BASE", "https://mainnet.zklighter.elliot.ai").rstrip("/")
   ACCOUNT_INDEX = int(os.environ["LIGHTER_ACCOUNT_INDEX"])
   API_KEY_INDEX = int(os.environ["LIGHTER_API_KEY_INDEX"])
   API_KEY = (
       os.environ.get("LIGHTER_API_SECRET")
       or os.environ.get("LIGHTER_API_KEY_PRIVATE_KEY")
       or os.environ["LIGHTER_API_KEY"]
   ).removeprefix("0x")
   MARKET_ID = 1  # 1=BTC perp, 0=ETH perp
   DISCOUNT = 0.10
   NOTIONAL_USD = 50.0

   root = pathlib.Path("/tmp/lighter-python/lighter/signers")
   signer_path = root / (
       "lighter-signer-darwin-arm64.dylib" if platform.system() == "Darwin"
       else "lighter-signer-linux-amd64.so"
   )

   class StrOrErr(ctypes.Structure):
       _fields_ = [("str", ctypes.c_char_p), ("err", ctypes.c_char_p)]

   class SignedTx(ctypes.Structure):
       _fields_ = [
           ("txType", ctypes.c_uint8),
           ("txInfo", ctypes.c_char_p),
           ("txHash", ctypes.c_char_p),
           ("messageToSign", ctypes.c_char_p),
           ("err", ctypes.c_char_p),
       ]

   signer = ctypes.CDLL(str(signer_path))
   signer.CreateClient.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int, ctypes.c_int, ctypes.c_longlong]
   signer.CreateClient.restype = ctypes.c_char_p
   signer.CreateAuthToken.argtypes = [ctypes.c_longlong, ctypes.c_int, ctypes.c_longlong]
   signer.CreateAuthToken.restype = StrOrErr
   signer.SignCreateOrder.argtypes = [
       ctypes.c_int,
       ctypes.c_longlong,
       ctypes.c_longlong,
       ctypes.c_int,
       ctypes.c_int,
       ctypes.c_int,
       ctypes.c_int,
       ctypes.c_int,
       ctypes.c_int,
       ctypes.c_longlong,
       ctypes.c_longlong,
       ctypes.c_int,
       ctypes.c_longlong,
   ]
   signer.SignCreateOrder.restype = SignedTx
   signer.SignCancelOrder.argtypes = [ctypes.c_int, ctypes.c_longlong, ctypes.c_longlong, ctypes.c_int, ctypes.c_longlong]
   signer.SignCancelOrder.restype = SignedTx

   chain_id = 304 if "mainnet" in HTTP_BASE else 300
   err = signer.CreateClient(HTTP_BASE.encode(), API_KEY.encode(), chain_id, API_KEY_INDEX, ACCOUNT_INDEX)
   if err:
       raise SystemExit(err.decode())

   token_res = signer.CreateAuthToken(int(time.time()) + 600, API_KEY_INDEX, ACCOUNT_INDEX)
   if token_res.err:
       raise SystemExit(token_res.err.decode())
   token = token_res.str.decode()

   nonce = requests.get(
       f"{HTTP_BASE}/api/v1/nextNonce",
       params={"account_index": ACCOUNT_INDEX, "api_key_index": API_KEY_INDEX},
       timeout=10,
   ).json()["nonce"]

   book = requests.get(
       f"{HTTP_BASE}/api/v1/orderBookDetails",
       params={"market_id": MARKET_ID},
       timeout=10,
   ).json()["order_book_details"][0]
   price_decimals, size_decimals = int(book["price_decimals"]), int(book["size_decimals"])
   last_price = float(book["last_trade_price"])
   limit_price = last_price * (1 - DISCOUNT)
   price_int = int(round(limit_price * (10 ** price_decimals)))
   size_int = int(
       max(
           round((Decimal(str(NOTIONAL_USD)) / Decimal(str(limit_price))) * (10 ** size_decimals)),
           round(Decimal(book["min_base_amount"]) * (10 ** size_decimals)),
       ),
   )

   client_order_index = int(time.time_ns() // 1_000_000)
   create = signer.SignCreateOrder(
       MARKET_ID,
       client_order_index,
       size_int,
       price_int,
       0,
       0,
       1,
       0,
       0,
       -1,
       nonce,
       API_KEY_INDEX,
       ACCOUNT_INDEX,
   )
   if create.err:
       raise SystemExit(create.err.decode())
   requests.post(
       f"{HTTP_BASE}/api/v1/sendTx",
       files={
           "tx_type": (None, str(int(create.txType))),
           "tx_info": (None, create.txInfo.decode()),
           "price_protection": (None, "true"),
       },
       timeout=15,
   )

   cancel_nonce = requests.get(
       f"{HTTP_BASE}/api/v1/nextNonce",
       params={"account_index": ACCOUNT_INDEX, "api_key_index": API_KEY_INDEX},
       timeout=10,
   ).json()["nonce"]
   cancel = signer.SignCancelOrder(MARKET_ID, client_order_index, cancel_nonce, API_KEY_INDEX, ACCOUNT_INDEX)
   if cancel.err:
       raise SystemExit(cancel.err.decode())
   requests.post(
       f"{HTTP_BASE}/api/v1/sendTx",
       files={"tx_type": (None, str(int(cancel.txType))), "tx_info": (None, cancel.txInfo.decode())},
       timeout=15,
   )
   ```

3. Use the token for private reads (`Authorization: <token>` or `auth=<token>` query) when hitting
   `accountActiveOrders`. `/api/v1/account` currently works without auth; `nextNonce` also succeeds
   without auth.

## Live probe (2025-12-10, mainnet, SDK-free)

- Signer: `lighter-signer-darwin-arm64.dylib` via `ctypes` + `requests` (no lighter-python install).
- Nonces consumed sequentially via `/nextNonce` (13 → 16); `sendTx` calls unauthenticated.
- Auth behaviour confirmed: `accountActiveOrders` rejects missing token (HTTP 400/code `20001`) but
  accepts either header or `auth` query. `account` and `nextNonce` succeeded without auth.

**BTC-USD-PERP (market_id=1, 10% below last)**

- `price_decimals=1`, `size_decimals=5`, `last_price≈92,299.5`, `limit_price≈83,069.6`
- `price_int=830696`, `size_int=60` (~$50 notional, min base 0.00020)
- Create: `code=200`, tx_hash=`78f13c839a0f10c1f6db19701dd45bc26f4016a61a492bd3c1a619137d38c5985209f866540c95bd`
- Cancel (~6s later): `code=200`, tx_hash=`169778979183122f75f2fb8af5bbf7944a9b3918b5099f3680da04eaaa5a0286a04aad3df8a22998`
- accountActiveOrders after cancel still showed two pre-existing bids (client_order_index
  `1765388099432`/`1765388052824`) but no new probe order.
- **Cleanup (same session):** legacy BTC bids canceled successfully via signer:
  - order_index `844423989556373` → tx_hash `42cf7ee7bad59fefc052a39a4c6a586d30140cd36d78836e15adb43dde510c48f5477cb9ebd62d2c`
  - order_index `844423989563098` → tx_hash `d1136fcf8829ecdbefd0121790af0e356497848a0e93c3c48c4b47b6efdb6fb4a679a8fb1cc7a0c3`
  - `accountActiveOrders` now empty for market 1.

**ETH-USD-PERP (market_id=0, 10% below last)**

- `price_decimals=2`, `size_decimals=4`, `last_price≈3,359.73`, `limit_price≈3,023.76`
- `price_int=302376`, `size_int=165` (~$50 notional, min base 0.0050)
- Create: `code=200`, tx_hash=`94a41a32be4374dd77ef5e0dff075503be648814e043a8b05011604c8852567b9393549eabe5b7d8`
- Cancel: `code=200`, tx_hash=`ee908b42c71284f28df38aa8d578e9ef7ee2d131517bd70e043f3e1b412b6864d5c3553c2d1d2969`
- accountActiveOrders empty before/after.

## Mainnet validation spike runbook (BTC & ETH perps)

> **Important**: Manual probe only; the adapter must not hardcode any of these numbers.

**Preconditions**

- `.env` populated with `LIGHTER_HTTP_BASE/WS_BASE`, `LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX`,
  and either `LIGHTER_API_SECRET` or `LIGHTER_API_KEY`.
- Signer binary available locally (e.g.,
  `/tmp/lighter-python/lighter/signers/lighter-signer-darwin-arm64.dylib`); chain_id `304` for
  mainnet.
- Repo venv active (`. .venv/bin/activate`).

**Step 1 — Sanity check**

- Load env:
  `set -a; source .env; set +a; echo "http_base=$LIGHTER_HTTP_BASE account_index=$LIGHTER_ACCOUNT_INDEX api_key_index=$LIGHTER_API_KEY_INDEX"`
- Verify public REST: `curl -s "$LIGHTER_HTTP_BASE/api/v1/orderBooks" | head -c 256`

**Step 2 — BTC probe (~$50, 10% below best ask)**

- Use the SDK-free snippet above with `MARKET_ID=1`.
- Expect `code=200` and `tx_hash` for both create and cancel; message includes `ratelimit`:
  `"didn't use volume quota"`.

**Step 3 — ETH probe (~$50, 10% below best ask)**

- Repeat Step 2 with `MARKET_ID=0`, `price_decimals=2`, `size_decimals=4`, `min_base_amount=0.0050`.

**Step 4 — Capture data for fixtures/PR3**

- Save raw HTTP interactions for `orderBookDetails`, `sendTx` (create/cancel), `account`,
  `accountActiveOrders` (with token), and `nextNonce` under `tests/test_data/lighter/http/`
  (redacted). **Captured:** mainnet fixtures added under `tests/test_data/lighter/http/mainnet_*.json`.
- Optional: capture WS traffic around order lifecycle. **Captured:** public stream for BTC place/cancel
  at `tests/test_data/lighter/ws/mainnet_order_book_place_cancel_btc.json`.

## Open items

- Persist redacted HTTP/WS captures (private endpoints + sendTx) into `tests/test_data/lighter/{http,ws}/`
  and cross-link from `03_LIGHTER_API_SPEC.md`.
- Add token refresh/retry logic (deadline-based) to the adapter implementation.
- Confirm private WS auth flow (token placement + schema) once WS access is exercised.
