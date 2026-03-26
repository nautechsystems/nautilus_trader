# TokenMM Trades Blotter Cutover

This runbook removes retained legacy TokenMM trade rows from Redis after the
normalized quantity contract is deployed and verified. The goal is to stop old
bare-`qty` rows from keeping `compatibility_mode` and `trade_gap` recovery
active forever.

Use this only after the Task 3 quantity-contract fixes are live and verified for
new writes.

## When to run this

Run the cutover when all of the following are true:

- new TokenMM trades are publishing `qty`, `qty_base`, `qty_venue`,
  `qty_conversion_status`, and `qty_conversion_source`
- `/api/v1/trades?profile=tokenmm&contract_version=2` shows normalized fields on
  freshly written rows
- remaining compatibility mode is caused only by retained historical Redis trade
  rows
- operators can tolerate losing pre-cutover Redis blotter history from the live
  stream

Do not run this while TokenMM writers are still producing legacy rows.

## What this changes

- deletes the live Redis trade stream keys for TokenMM strategy IDs
- does not mutate SQLite or other persistent telemetry stores
- allows the live Redis stream to rebuild from new normalized writes only

This is a cutover, not a migration. Historical live-stream rows are discarded
rather than reinterpreted.

## Preconditions

1. Schedule a short TokenMM maintenance window.
2. Stop or quiesce TokenMM trade writers so no new rows are appended during the
   reset.
3. Capture the active TokenMM strategy IDs from:

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
```

4. Confirm the Redis key pattern for each strategy ID:

```text
flux:v1:trades:stream:<strategy_id>
```

Adjust the namespace or schema version if production differs from `flux:v1`.

## Backup

Take a bounded backup of each TokenMM trade stream before deletion. At minimum,
record:

- `XLEN <key>`
- `XINFO STREAM <key>`
- a bounded `XRANGE` or `XREVRANGE` sample

Example:

```bash
redis-cli XLEN 'flux:v1:trades:stream:maker_v3_01'
redis-cli XINFO STREAM 'flux:v1:trades:stream:maker_v3_01'
redis-cli XREVRANGE 'flux:v1:trades:stream:maker_v3_01' + - COUNT 20
```

If you need full recovery, export the stream contents before deletion using your
standard Redis backup process.

## Cutover

1. Stop or pause TokenMM trade writers.
2. Confirm no new entries are arriving on the target stream keys.
3. Delete only the TokenMM trade stream keys.

Example:

```bash
redis-cli DEL 'flux:v1:trades:stream:maker_v3_01'
redis-cli DEL 'flux:v1:trades:stream:maker_v3_02'
```

4. Resume TokenMM trade writers.
5. Reconnect Fluxboard clients or restart the Flux API process if you want all
   socket consumers to re-prime immediately.

## Verification

Run these checks after writers resume:

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/trades?profile=tokenmm&contract_version=2'
curl -fsS 'http://127.0.0.1:5022/api/v1/trades/delta?profile=tokenmm&contract_version=2&since_seq=0'
```

Verify:

- new rows include normalized quantity fields
- `compatibility_mode` is absent or `false`
- the trades page no longer enters repeated `trade_gap` recovery for the same
  unchanged legacy condition
- Fluxboard `tokenmm.trades` settles to `LIVE` after the first fresh snapshot

## Rollback

If the deployment is bad and you must roll back:

1. Stop TokenMM trade writers again.
2. Restore the saved Redis stream data from backup, or redeploy the previous app
   version and accept that the live stream will rebuild only from newly written
   rows.
3. Re-run the verification checks above.

Do not restore legacy rows into a normalized deployment unless you also accept
that `compatibility_mode` and recovery gating may return.
