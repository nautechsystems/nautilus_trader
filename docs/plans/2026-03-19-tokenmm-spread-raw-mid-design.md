# TokenMM Spread Raw Mid Design

**Goal:** Make the TokenMM `Spread` field mean raw maker market mid vs reference/FV mid, without conflating it with quote skew or placement.

## Decision

Use option 1:

- `spread_net_bps` becomes raw maker-top mid vs ref/FV mid for `maker_v3`
- `decision_edge_bps` remains quote/decision-oriented
- `edge2_bps` remains derived from `decision_edge_bps` and `required_edge_bps`

## Rationale

Operators use `Spread` to answer "is the venue cheap or rich?" The current `maker_v3` override answers a different question: "where did our quoted mid land after skew and placement?" That mixes venue basis with inventory translation and makes the column misleading during live risk management.

The skew and quote translation are already available elsewhere:

- `pricing_adjustments[].skew_bps_signed`
- `maker_v3.quote_snapshot.place_bid/place_ask`
- `decision_edge_bps`

That means `Spread` does not need to carry both meanings.

## Behavior

- Raw spread source:
  - maker market mid from `maker_top_bid/maker_top_ask` when available
  - fallback to visible maker-leg market data when quote snapshot maker-top is absent
- Reference/FV source:
  - reference mid from `ref_bid/ref_ask`
  - fallback to visible ref-leg or `fv_row.fv` as today
- Quoted spread remains implicit in `decision_edge_bps` when no explicit backend value is published

## Testing

- API payload regression proves `spread_net_bps` stays raw while `decision_edge_bps` stays quoted for `maker_v3`
- Fluxboard signal table regression proves the `Spread` cell renders raw market-vs-FV, not quote-vs-FV
