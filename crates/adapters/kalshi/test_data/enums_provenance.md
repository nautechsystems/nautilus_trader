# Enum Fixture Provenance

`enums.json` records the allowed values from the official Predictions
[REST schema](https://docs.kalshi.com/openapi.yaml) and
[WebSocket schema](https://docs.kalshi.com/asyncapi.yaml), retrieved on 5 November 2026.
The fixture records each schema version, SHA-256 hash, and enum location as a JSON Pointer.
It covers all 95 allowed enum declarations, the `metadata_updated` event constant, four command
names, and 34 server message types. The `not` constraint rejects `unit: cents` with `value >= 100`;
it defines no additional allowed enum.

The REST schema bytes differ from the earlier 5 November retrieval in
[REST fixture provenance](rest_provenance.md), while retaining version `3.36.0`.

The REST `CountFilterQuery` description lists the position count filters. The `/account/limits`
description lists public API usage tiers, and `ApiUsageLevelGrant.source` describes grant sources.
These documented values appear in the fixture alongside the schema enum sets. Shared types
include the combined REST and WebSocket values where the protocols differ. Nullable fee overrides
use the existing field wrapper; JSON null is not an enum variant.

The data frames contain independently authored synthetic subscription IDs, sequence numbers,
timestamps, and empty payloads. They check message decoding and channel identity. They do not
establish support for subscribing to every documented channel. No SDK code, packaged fixtures,
credentials, or account data are copied.
