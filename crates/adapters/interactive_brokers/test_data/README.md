# Interactive Brokers Test Fixtures

`order_report_responses.json` contains synthetic frames constructed from the pinned `ibapi 5.0.0`
[protobuf definitions](https://docs.rs/crate/ibapi/5.0.0/source/src/proto/protobuf.rs) and
[message IDs](https://docs.rs/crate/ibapi/5.0.0/source/src/messages.rs). These frames are not captured
TWS or Gateway responses. The dependency keeps its protobuf types and stub client private, so the
test sends these frames through its public TCP client.

Each response includes its length prefix and protobuf message ID, which includes the protocol's
offset of 200:

| Request          | Outgoing ID | Incoming IDs  |
| ---------------- | ----------- | ------------- |
| Start API        | 271         | 209, 215      |
| All open orders  | 216         | 205, 205, 253 |
| Completed orders | 299         | 301, 302      |

The entry `PERM-10` is a filled buy market order completed at `20260924 12:02:04 UTC`. The open
exits `PERM-11` and `PERM-12` are a sell stop and sell limit. Both reference parent order 1 and
client 7, and share the `EXIT-1` OCA group with reduce-with-block behavior. The account `DU123`
is synthetic. The terminal-response negative case changes the first exit's status from
`Submitted` to `Cancelled` without changing the frame length.

This fixture proves collection, filtering, and linking against the pinned client. It does not
establish conformance with real venue payloads. Captured venue responses are still required to
meet the adapter guide's valid-fixture sourcing requirement and to validate the full wire format.
