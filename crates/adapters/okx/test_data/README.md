# OKX HTTP fixtures

`http_get_account_configuration.json` follows the response example in the
[official account-configuration documentation](https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-account-configuration),
retrieved on 2026-09-08. The account IDs are synthetic. The HTTP tests vary the
five configuration fields to exercise documented values and malformed inputs.

`http_get_trade_fee_grouped_response.json` is the complete response example from
the [official fee-rates documentation](https://my.okx.com/docs-v5/en/#trading-account-rest-api-get-fee-rates),
retrieved on 2026-10-04. It includes legacy scalar rates and a `feeGroup` entry
with both `rpiMaker` and `elpMaker`.

`http_get_instruments_spot_group_id.json` is the complete response example from
the [official public-instruments documentation](https://my.okx.com/docs-v5/en/#public-data-rest-api-get-instruments),
retrieved on 2026-10-04. It includes the SPOT instrument's `groupId` alongside
`instCategory` and the deprecated `category` field.

These two documented examples are unchanged apart from whitespace. Tests also
derive synthetic compatibility cases from them: omitted scalar rates, distinct
RPI/ELP rates, unavailable and zero rates, and missing, empty, or alternate group
IDs. Those variants are not venue captures.

`http_error_invalid_access_key.json` is the HTTP 401 body OKX returned on
2026-09-29 for `GET /api/v5/account/balance` with an invalid API key. OKX omits
`data` from this response.

`http_error_not_found.json` is the HTTP 404 body OKX returned on 2026-09-29 for
an unknown `/api/v5` path. Its `code` is a number, unlike the string codes in
OKX API error responses.
