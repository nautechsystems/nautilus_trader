# Third-Party Licenses (Derive adapter)

This crate references third-party material for action-signing equivalence testing.

- **Derive.xyz: `derive-py`**
  - Usage: The Rust EIP-712 signing pipeline under `src/signing/` independently implements
    Derive's published v3 action-signing protocol. The official Python SDK generates
    `test_data/common/signing_trade_action_vectors.json` through
    `scripts/oracle-py/derive/generate_oracle.py`. These generated outputs provide an
    independent equivalence oracle; the fixture records its source revision, source hashes,
    dependency versions, and regeneration procedure.
  - Pinned revision: `fad785e6c328746b5f8a8219e14009670bc97a35` (version 0.1.4).
  - Attribution: Copyright (c) 2026 derive-py contributors.
  - License: MIT, recorded in the upstream `LICENSE` file.
  - Source: <https://github.com/derivexyz/derive-py>.

- **Derive.xyz: `v2-action-signing-python`**
  - Usage: Published session-key and owner test inputs remain in the signing tests,
    benchmarks, and oracle generator. The v3 oracle uses `derive-py`.
  - Pinned revision: `d1914d61985e33559244da242892c7255b6fd0ca` (version 0.0.13,
    committed 2025-08-21).
  - Attribution: Derive.xyz <joshua@derive.xyz>, 8baller <8baller@station.codes>
    (authors declared in the upstream `pyproject.toml`).
  - License: MIT (declared via the pyproject classifier; the upstream repository
    carries no LICENSE file at the pinned revision).
  - Source: <https://github.com/derivexyz/v2-action-signing-python>.

The MIT notice for `derive-py` follows:

> Copyright (c) 2026 derive-py contributors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.
