# Fixture Source

The `settlement_maker_buy` and `settlement_maker_sell` fixtures come from paired account user-stream
and targeted REST captures on Polymarket mainnet on 2026-11-16. Each trade includes two owned maker
orders and other participants. The market uses CTF (Gamma version `v1`), is not negative-risk, and
has a tick size of `0.01`. The [provider results](https://github.com/nautechsystems/nautilus_trader/issues/4876#issuecomment-6916171699)
describe the capture scope.

The files without `_rest` contain the first `MATCHED` user-stream trade. The `_rest` files contain
the independently captured `CONFIRMED` REST trade, rather than a conversion of the stream message.
Prices, quantities, maker sets, sides, statuses, and transport-specific fields are preserved.

Account, order, trade, transaction, and market identities use consistent placeholders. Token IDs map
to the test instruments `1000` (No) and `1001` (Yes). Match times reset to 2025-01-01 UTC. The owned
wallet uses a public test address. These samples do not establish guarantees for other trade classes
or historical fee authority.
