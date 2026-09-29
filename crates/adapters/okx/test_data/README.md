# OKX HTTP fixtures

`http_get_account_configuration.json` follows the response example in the
[official account-configuration documentation](https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-account-configuration),
retrieved on 2026-09-08. The account IDs are synthetic. The HTTP tests vary the
five configuration fields to exercise documented values and malformed inputs.

`http_error_invalid_access_key.json` is the HTTP 401 body OKX returned on
2026-09-29 for `GET /api/v5/account/balance` with an invalid API key. OKX omits
`data` from this response.

`http_error_not_found.json` is the HTTP 404 body OKX returned on 2026-09-29 for
an unknown `/api/v5` path. Its `code` is a number, unlike the string codes in
OKX API error responses.
