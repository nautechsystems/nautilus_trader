# Adapters and Integrations

This document describes how NautilusTrader classifies adapters, the path between
tiers, and how community adapters can be listed.

For naming rules, disclaimer requirements, and trademark guidance, see [TRADEMARK.md](./TRADEMARK.md).

## Adapter tiers

### Official

Maintained in the [nautilus_trader](https://github.com/nautechsystems/nautilus_trader)
repository or the [nautechsystems](https://github.com/nautechsystems) GitHub
organization, and designated as official by project maintainers. Documented and
supported by maintainers.

**Requirements:** Strategic and technical fit, maintainability, demand, legal suitability,
alignment with the NautilusTrader domain model and standard adapter patterns.

### Community

Third-party adapter maintained by its own author and listed in this document
because it met the listing criteria at the time of review. Listing does not
constitute endorsement, support, or official status.

### External

Third-party adapter that exists outside the project and is not listed here as
Community. External adapters exist at their maintainer's discretion.

## Adoption path

Adapters can move between tiers. These paths are independent; community listing
is not a prerequisite for official adoption.

**External to Community.** The maintainer opens a GitHub issue or discussion
requesting a community listing. Maintainers review against the listing criteria.

**To Official.** The adapter maintainer or a community member opens an RFC
following the process in the
[ROADMAP](./ROADMAP.md#community-contributed-integrations). Maintainers evaluate
against the existing criteria: stability, demand, technical fit, bandwidth, and
alignment with Rust adapter patterns. If approved, the adapter is brought into
the core repository or the nautechsystems organization, re-licensed if needed,
and maintained to project standards.

### Demotion

Official adapters may be archived or handed over to a community maintainer if
they fall behind the core API or no longer justify ongoing maintenance relative
to demand, architectural fit, and project standards. An adapter moves to
Community only if a maintainer steps up; otherwise it becomes External or
archived.

### Rejected RFCs

A rejected RFC does not remove an adapter from the ecosystem. The adapter can
remain Community or External with the usual naming and disclaimer expectations
defined in [TRADEMARK.md](./TRADEMARK.md).

### Existing adapters

All adapters currently in the core repository are Official by definition. This
framework applies going forward.

## Support

NautilusTrader maintainers provide support for official adapters only. Issues
with community or external adapters should be directed to their respective
maintainers.

If you believe an issue originates in the NautilusTrader core rather than a
third-party adapter, please file a minimal reproducible example against the
[core repository](https://github.com/nautechsystems/nautilus_trader/issues).

## Official adapters

The following adapters are maintained in the core repository:

| Adapter             | Type           |
|---------------------|----------------|
| Architect (AX)      | Data/Execution |
| Betfair             | Data/Execution |
| Binance             | Data/Execution |
| BitMEX              | Data/Execution |
| Bybit               | Data/Execution |
| Coinbase            | Data/Execution |
| Databento           | Data           |
| Deribit             | Data/Execution |
| dYdX                | Data/Execution |
| Hyperliquid         | Data/Execution |
| Interactive Brokers | Data/Execution |
| Kraken              | Data/Execution |
| OKX                 | Data/Execution |
| Polymarket          | Data/Execution |
| Tardis              | Data           |

## Community listings

Community adapters listed here met the listing criteria at the time of review.

### Listing criteria

- Complies with [TRADEMARK.md](./TRADEMARK.md) (naming and disclaimer).
- Licensed under an open-source license *compatible* with LGPL v3.0.
- Maintainer is identifiable and contactable.
- Repository shows activity within the last six months.
- Repository includes installation or usage documentation.

### How to list

Open a GitHub issue or discussion with a link to the repository and a brief
description. Maintainers review against the listing criteria above and decide
whether to add the listing.

Maintainers may update or remove a community listing if it no longer meets the
listing criteria or misrepresents its relationship to NautilusTrader.

### Community adapters

Community adapters are externally maintained and are not supported by NautilusTrader maintainers.

| Project                                                 | Description                                | Maintainer |
|---------------------------------------------------------|--------------------------------------------|------------|
| [mt5-connect](https://github.com/aulekator/mt5-connect) | Unofficial community MetaTrader 5 adapter. | aulekator  |

## Updates

This document may be updated from time to time. Changes are tracked through the
repository's version control history.

Last updated: 2026-04-25
