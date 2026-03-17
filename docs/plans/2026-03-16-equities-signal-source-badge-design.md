# Equities Signal Source Badge Design

**Goal:** Show the configured reference source on the equities Fluxboard Signal page without changing other profiles or backend APIs.

## Scope

- Add a small source badge on the equities Signal page only.
- Show it on the reference leg only.
- Derive the label from the existing signal payload:
  - prefer `leg.route` when present
  - otherwise parse the suffix of `leg.instrument_id`
- Examples:
  - `AAPL.BLUEOCEAN` -> `BLUEOCEAN`
  - `AAPL.NASDAQ` -> `NASDAQ`

## Non-Goals

- Do not change non-equities Signal pages.
- Do not add backend quote-origin telemetry.
- Do not claim this is the true live quote origin; it is the configured route/source only.

## UI Shape

- Keep the current leg label.
- Add a small neutral badge beside the reference-leg label on `/equities/signal`.
- Hide the badge when no configured route/source can be derived.
