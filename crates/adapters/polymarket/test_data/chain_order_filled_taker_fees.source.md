# Fixture Source

`chain_order_filled_taker_fees.json` holds taker legs of Polymarket CTF Exchange V2 `OrderFilled`
events for a funded test account, read from Polygon mainnet logs on 2026-09-30. The events come
from the standard exchange (`0xE111180000d2663C0091e4f400237545B87B996B`) and the neg-risk
exchange (`0xe2222d279d744050d28e00520010520000310F59`), with fills from 2026-09-24 to
2026-09-26 UTC. The last seven vectors come from live validation fills on 2026-09-30 UTC, where
Nautilus reported the same quantities and commissions.

`maker_amount_filled`, `taker_amount_filled`, and `fee` are the event values unchanged, as
six-decimal wire mantissas. A BUY fills shares in `taker_amount_filled`; a SELL sells shares in
`maker_amount_filled`. Transaction hashes, addresses, and token IDs are omitted.

`price` is the trade price from the account's CLOB trade record for the same transaction, whose
`size` equals the event's share amount. `fee_rate` is the market's Gamma `feeSchedule.rate`
(exponent 1), or 0 for a market without fees. Both were read on 2026-09-30.
