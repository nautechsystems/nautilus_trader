# OKX HTTP fixtures

`http_get_account_configuration.json` follows the response example in the
[official account-configuration documentation](https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-account-configuration),
retrieved on 2026-09-08. The account IDs are synthetic. The HTTP tests vary the
five configuration fields to exercise documented values and malformed inputs.
