# Trademark and Naming Policy

This policy governs the use of the NautilusTrader™ name, related marks, and logos
by third-party projects, forks, adapters, wrappers, tutorials, courses, and any
other derivative or compatible works.

NautilusTrader is a registered trademark of Nautech Systems Pty Ltd
(ABN 88 609 589 237). Nautech Systems owns and maintains these marks and this
policy. The full trademark policy is published at
[nautilustrader.io/legal/trademark-policy](https://nautilustrader.io/legal/trademark-policy/).
Questions or concerns should be directed to <legal@nautechsystems.io>.

## The marks

The following names are reserved for official project use by Nautech Systems:

- **NautilusTrader** (combined form)
- **Nautilus Trader** (separated form)
- **nautilus_trader** (package name)
- **nautilus-trader** (hyphenated form)
- The NautilusTrader logo and any associated visual branding

These marks identify software produced and maintained by Nautech Systems. No
third-party project may use these marks as a prefix or leading component of a
project name, package name, or public registry identifier.

## General principles

1. **The marks exist to protect users from confusion about what is official and
   what is not. This policy aims to be practical and fair, not restrictive for
   its own sake.**

2. Third-party projects that reference NautilusTrader must clearly and honestly
   represent their relationship with the project.

3. Third-party projects that use or distribute NautilusTrader code must comply
   with the [LGPL v3.0 license](./LICENSE) where applicable. This trademark
   policy is separate from and in addition to the software license.

4. Compliance with this policy does not constitute endorsement, affiliation, or
   official status. Only projects maintained within the
   [nautechsystems](https://github.com/nautechsystems) GitHub organization and
   designated by project maintainers carry official status.

## Acceptable nominative use

Third-party projects may reference the NautilusTrader name to describe
compatibility or purpose. The following uses are acceptable:

- "for NautilusTrader"
- "compatible with NautilusTrader"
- "an adapter for NautilusTrader"
- "works with NautilusTrader"

**These phrases describe a relationship with the project. They are acceptable in
documentation, descriptions, and READMEs, but must not be used in project or
package names.**

## Where these rules apply

This policy applies to repository names, PyPI distribution names, crates.io
crate names, npm package names, and any other public package registry. It also
applies to domain names, GitHub organization and user names, and social media
handles.

## Naming rules for third-party projects

Third-party projects must not use `nautilustrader`, `nautilus_trader`, or
`nautilus-trader` as a prefix or leading component of a project or package name. The standalone
word `nautilus` is also restricted when used for trading, brokerage, market data,
backtesting, or related financial software where it is likely to create confusion
with NautilusTrader. Within that same domain, the `nautilus-*` namespace on
package registries is reserved for officially published NautilusTrader packages.

As a practical matter, source repositories usually create less confusion than
published packages. Projects that do publish to package registries should be
especially careful about naming. Contributors interested in official adapter
inclusion should follow the RFC process described in the
[ROADMAP](./ROADMAP.md#community-contributed-integrations).

**The `nt` shorthand.** The project designates `nt` as the approved shorthand
for third-party projects to signal NautilusTrader compatibility.

**Naming examples:**

| Compliant               | Not compliant            |
|-------------------------|--------------------------|
| `mt5-nt-community`      | `nautilus-mt5`           |
| `sinopac-nt-community`  | `nautilus-sinopac`       |
| `mt5-connect`           | `nautilustrader-stocks`  |

The `-community` suffix is recommended to clearly signal an independent project,
but it is not required.

## Required disclaimer

All third-party software projects that distribute code integrating with
NautilusTrader (adapters, wrappers, forks, packages, libraries) must include a
clear disclaimer in their README or primary documentation. The disclaimer must
state that the project is:

1. not affiliated with Nautech Systems Pty Ltd or the NautilusTrader project,
2. not endorsed by Nautech Systems Pty Ltd or the NautilusTrader project, and
3. not supported by Nautech Systems Pty Ltd or the NautilusTrader project.

**Reference text:**

> This is an independent community project. It is not affiliated with, endorsed
> by, or supported by Nautech Systems Pty Ltd or the official NautilusTrader
> project.

Projects may adapt the phrasing to fit their documentation style, but all three
elements must be present and the legal entity name (Nautech Systems Pty Ltd)
must appear.

## Forks

Forks created for personal use, internal non-public corporate use, or for the purpose of
contributing back to the official repository via pull request are exempt from the
naming, disclaimer, and branding requirements in this policy. Standard
development forks that follow the workflow described in
[CONTRIBUTING.md](./CONTRIBUTING.md) require no changes.

## Logos

The NautilusTrader logo and associated visual branding are proprietary to
Nautech Systems. Third-party projects must not use the official logo or
derivatives of it in a way that implies official status or endorsement. Use of
the logo requires prior written permission from Nautech Systems.

## Community channels

Access to, promotion within, or use of official NautilusTrader community
channels (including forums, chat servers, or mailing lists) for commercial
products or services requires prior written approval from Nautech Systems.
Participation in community channels does not imply endorsement or affiliation.

## Use by partners and related entities

Partners may use the marks under the terms of a separate written partnership or
co-branding agreement with Nautech Systems. Related entities may use the marks
only under a separate written license or authorization from Nautech Systems.

## Enforcement

Nautech Systems reserves the right to enforce its marks through appropriate
means, including but not limited to: requesting name changes, requesting removal
of confusing branding, delisting from official channels, and pursuing formal
trademark remedies where necessary.

Enforcement will ordinarily begin with direct notice and an opportunity to cure
before stronger measures are considered. A decision not to enforce against a
particular use does not constitute a waiver of the right to enforce against that
or any other use in the future.

## Updates

This policy may be updated from time to time. The authoritative version is
published at
[nautilustrader.io/legal/trademark-policy](https://nautilustrader.io/legal/trademark-policy/).
Both versions are maintained to match in substance.

Last updated: 2026-04-13
