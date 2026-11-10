# REST Fixture Provenance

`rest_requests.json`, `rest_responses.json`, and `rest_numbers.json` contain independently authored
synthetic test data. They contain no venue captures, credentials, account data, SDK source, or SDK fixtures.

The request and response field contracts follow the [official REST schema](https://docs.kalshi.com/openapi.yaml),
retrieved on 16 October 2026: version `3.34.0`, SHA-256
`a33d05aaa5deab319674407d543b5c0027d41adbf2c4f8a11dab708a0debd35a`.
The response file covers the implemented discovery, data, historical, account, order, batch, and error
envelopes, including empty pages. Its identifiers, amounts, timestamps, and error text are original test inputs.

`rest_numbers.json` supplies original decimal boundary inputs for exact numeric decoding without a binary
floating-point conversion. These fixtures establish deterministic protocol behavior; they do not establish
venue acceptance of order requests.

`quota_responses.json` contains original synthetic rate-limit, duplicate-identity, and malformed-success
responses. The rate-limit body follows the [official token-limit guide](https://docs.kalshi.com/getting_started/rate_limits),
retrieved on 17 October 2026. The duplicate response is an adversarial test input, not a captured venue guarantee.

`history.json` combines the public market, event, and series definitions described in
[instrument fixture provenance](instruments_provenance.md) with original synthetic trade, cutoff,
and candle inputs based on the history field contracts below. Tests change timestamps, quantities, prices,
identities, cursor values, and nullable fields to exercise routing and parsing boundaries. These changes
are controlled test inputs, not venue observations. No SDK source or packaged fixtures are used.

History field contracts follow the official REST schema retrieved on 18 October 2026: version `3.34.0`, SHA-256
`bd0c218a65b79f5ee2a27689a5b1d902b6df67663864373bf9e9980f0f9ad9ee`, and the
[historical-data guide](https://docs.kalshi.com/getting_started/historical_data).
The schema payload differs from the 16 October retrieval while retaining the same version.

The synthetic daily-offset case uses the `2026-08-16T04:00:00Z` end timestamp observed on 18 October 2026
from `/historical/markets/KXHIGHNY-26AUG16-T85/candlesticks?period_interval=1440`. The timestamp
establishes that a daily candle need not end at UTC midnight; the test retains synthetic OHLCV values.

The `native_*`, `limit_matrix`, and `quota_batch` request cases, `reject_*` response cases, and
`batch_*_cost` entries retain synthetic payloads extracted from tests. Tests replace identities and
matrix values explicitly. Rejection text and endpoint costs are controlled inputs, not venue observations.
The request, error, and endpoint-cost field contracts are checked against the official REST schema
retrieved on 5 November 2026: version `3.36.0`, SHA-256
`94a3577612d78883c19fa33a3173e70b79c0f3cd765529d4ce3276bce7e5b884`.
`quota_batch` deliberately omits order fields because its test exercises dispatch admission only.
