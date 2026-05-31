# nautilus-lighter

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-lighter)](https://docs.rs/nautilus-lighter/latest/nautilus-lighter/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-lighter.svg)](https://crates.io/crates/nautilus-lighter)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Lighter](https://lighter.xyz) decentralized spot and perpetuals exchange.

The `nautilus-lighter` crate implements the Lighter adapter for NautilusTrader, including
typed HTTP and WebSocket clients, REST and stream models, venue parsing, data and execution
client wiring, and an in-tree L2 signer for the official **Lighter API**.

Lighter is a high-throughput central-limit-order-book decentralized crypto exchange
for spot and perpetual futures, settling on Ethereum via a custom zero-knowledge rollup.
Order matching is performed off-chain by a sequencer, with ZK proofs published on-chain
to guarantee correctness of matching, fills, and liquidations.

Trading is non-custodial: users hold their assets in Lighter's smart contracts
and authorise trades with their own keys.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module.

Python bindings are intentionally narrow: configuration, enums, factory wiring, and the
integrator revocation helper. Data and execution clients are consumed directly through the Rust
trait surface.

[High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.

## Integrator attribution

Submitted create and modify order transactions carry the NautilusTrader integrator account index in
Lighter's `L2TxAttributes`. This helps us gauge real usage of the integration and prioritize
ongoing maintenance. Maker and taker integrator fees are set to zero, so attribution adds no trading
cost.

Lighter requires an `ApproveIntegrator` approval before these attributes can be attached to orders.
During startup, the execution client submits the required **zero-fee** approval for the configured
L2 account. See the
[Lighter integration guide](https://nautilustrader.io/docs/nightly/integrations/lighter.html#integrator-attribution)
for approval and revocation details.

## L2 transaction signer

The crate ships an in-tree implementation of the Lighter L2 signer (Schnorr
over the ECgFp5 curve, Goldilocks field, Poseidon2 binding). Correctness is
gated by independent layers:

- **Vector parity** with the upstream Go reference (`elliottech/poseidon_crypto`)
  for every algebra layer.
- **Compiled-signer oracle parity** with the signer distributed by the official
  `lighter-python` SDK, covering end-to-end outputs for the four supported tx
  kinds.
- **Differential parity** with Thomas Pornin's MIT-licensed Rust reference
  (`pornin/ecgfp5`), pulled in as a zero-transitive-dep `#[cfg(test)]`
  dev-dep. Every public algebra operation is asserted byte-for-byte against
  it under proptest, with a coverage-guided fuzz soak in
  [`fuzz/`](fuzz/README.md) for continuous validation. Pornin's reference
  accompanies the curve's design paper (IACR ePrint 2022/274), has been
  public and reused by downstream zero-knowledge projects since 2022, and
  shares no code lineage with our implementation: a bug that slips the
  gate would have to be present in both implementations in the same way.
- **Property tests** covering ring axioms, group laws, and Frobenius
  identities on the cryptographic primitives.

Round-trip testing against Lighter itself remains the final correctness
gate for what the sequencer accepts.

## Documentation

See [the docs](https://docs.rs/nautilus-lighter) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

Reference attributions for cryptographic parameter sets and reproduced test vectors
used by the L2 transaction signer are listed in
[`licenses/THIRD_PARTY_LICENSES.md`](licenses/THIRD_PARTY_LICENSES.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
