# nautilus-lighter

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-lighter)](https://docs.rs/nautilus-lighter/latest/nautilus_lighter/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-lighter.svg)](https://crates.io/crates/nautilus-lighter)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Lighter](https://lighter.xyz) decentralized spot and perpetuals exchange.

The `nautilus-lighter` crate implements the Lighter protocol adapter for NautilusTrader, including
typed HTTP and WebSocket clients, REST and stream models, venue parsing, data and execution
client wiring, and an in-tree L2 signer for the official **Lighter API**. It supports the Lighter
and Robinhood Chain deployments with deployment-specific endpoints, chain IDs, settlement
currencies, and Nautilus venue defaults.

Lighter is a high-throughput central-limit-order-book decentralized crypto exchange
for spot and perpetual futures, settling on Ethereum via a custom zero-knowledge rollup.
Order matching is performed off-chain by a sequencer, with ZK proofs published on-chain
to guarantee correctness of matching, fills, and liquidations.

Trading is non-custodial: users hold their assets in Lighter's smart contracts
and authorize trades with their own keys.

The official Lighter API reference can be found at <https://docs.lighter.xyz/api>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Configuration

See the [integration guide](../../../docs/integrations/lighter.md) for client settings,
credentials, and connection setup.

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables libFuzzer integration for fuzz targets.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

Python bindings are intentionally narrow: configuration, enums, factory wiring, and the
integrator revocation operation. Data and execution clients are consumed directly through the Rust
trait surface.

## Integrator attribution

On Lighter and Robinhood mainnets, the execution client adds NautilusTrader's integrator account index
to create and modify transactions with zero integrator fees. At startup it attempts to approve the
integrator and apply the `NAUTILUS` referral code. Both testnets omit attribution.

See [attribution details](../../../docs/integrations/lighter.md#integrator-attribution)
for approval, maker-only keys, and revocation.

## L2 transaction signer

The crate ships an in-tree implementation of the Lighter L2 signer (Schnorr
over the ECgFp5 curve, Goldilocks field, Poseidon2 binding). Correctness is
gated by independent layers:

- **Vector parity** with the upstream Go reference (`elliottech/poseidon_crypto`)
  for every algebra layer.
- **Compiled-signer oracle parity** with `lighter-go` v1.0.12, covering end-to-end outputs for
  create, cancel, modify, cancel-all, leverage, integrator approval, and grouped orders.
- **Differential parity** with Thomas Pornin's MIT-licensed Rust reference
  (`pornin/ecgfp5`), kept in the publish=false fuzz crate so the crates.io
  package graph has no git dependencies. The fuzz targets assert every public
  algebra operation byte-for-byte against it and soak point decode and scalar
  multiplication under coverage guidance. Pornin's reference accompanies the
  curve's design paper (IACR ePrint 2022/274), has been public and reused by
  downstream zero-knowledge projects since 2022, and shares no code lineage
  with our implementation: a bug that slips the gate would have to be present
  in both implementations in the same way.
- **Property tests** covering ring axioms, group laws, and Frobenius
  identities on the cryptographic primitives.

Round-trip testing against Lighter itself remains the final correctness
gate for what the sequencer accepts.

Measured signing cost, including a comparison with the official Go SDK, is
recorded in [`benches/BENCHMARKS.md`](benches/BENCHMARKS.md).

## Fuzzing

Coverage-guided fuzz targets for the L2 signer live in [`fuzz/`](fuzz/README.md). They require the
workspace-pinned `cargo-fuzz` binary and a Rust nightly toolchain; install them from the repository
root with `make install-tools` and `rustup toolchain install nightly`.

## Documentation

- [Rust API reference](https://docs.rs/nautilus-lighter)
- [Integration guide](../../../docs/integrations/lighter.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/lighter/)

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
