# Lighter Rust Data Client Starter

This quickstart builds a minimal Rust `LiveNode` with the Lighter data client and the built-in
`DataTester` actor. `cargo run` connects to Lighter testnet public streams.

## Run

```bash
cargo run
```

Stop the node with Ctrl+C.

The quickstart uses public data only. Execution clients and live order submission need the credential
variables documented in the Lighter integration guide.

## Next steps

- Change `INSTRUMENT_ID` to another Lighter market such as `ETH-PERP.LIGHTER`.
- Enable `subscribe_book_deltas(true)` and `manage_book(true)` in `DataTesterConfig` for L2 MBP data.
- Add an execution client only after the data path is working.
