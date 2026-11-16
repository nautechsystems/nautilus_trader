# Kalshi Adapter Benchmarks

Numbers measured 2026-11-01 on AMD Ryzen Threadripper 9980X, Linux 7.0.0-38-generic,
under rustc 1.99.1 (58cf8f9c3 2026-10-13), `bench-lto` profile (release opts +
`lto = "fat"` + `codegen-units = 1`, `debug = full`). The CPU governor was
`performance` for the measured session and restored to `powersave` afterward. ASLR
was disabled with `setarch -R`. The build used `CARGO_BUILD_JOBS=8`. The measured
Kalshi adapter code matches this branch. The bench harness was uncommitted.

Each Criterion cell is the median of three session estimates. Each session estimate
is Criterion's median from 100 samples after a 3 second warmup. Throughput is from
the same session as that median. Book and dispatch rows were measured again after
excluding prepared-state destruction. Quote, trade, submit, and decoder rows are
from the earlier three sessions. There is no latency target. Absolute numbers vary
by machine; only same-machine deltas are meaningful.

Refresh on a substantive performance change or before release.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
CARGO_BUILD_JOBS=8 setarch "$(uname -m)" -R cargo bench --profile bench-lto -p nautilus-kalshi \
    --bench data --bench exec --bench micros
CARGO_BUILD_JOBS=8 setarch "$(uname -m)" -R cargo bench --profile bench-lto -p nautilus-kalshi \
    --bench alloc
sudo cpupower frequency-set -g powersave
```

The `alloc` bench prints requested allocation counts. It is not a Criterion
target. For policy and the general noise-reduction recipe see
[`BENCHMARKING.md`](../../../../BENCHMARKING.md) at the repo root.

## Inbound pipeline (`data.rs`)

Raw WebSocket frame bytes to a Nautilus domain value. Quotes and trades measure
`decode_frame` plus the data client's quote or trade parse. Book rows measure
`decode_frame` plus `BookSyncTracker::event`. Instrument setup and subscription
confirmation stay outside the timed region. Book and dispatch rows use
`iter_batched_ref`, so dropping the prepared state stays outside the timed region.

| Bench                            | Median  | Throughput |
| -------------------------------- | ------- | ---------- |
| `inbound_pipeline/quotes`        | 926 ns  | 1.08 M/s   |
| `inbound_pipeline/trades`        | 1.11 µs | 897 k/s    |
| `inbound_pipeline/book_snapshot` | 2.38 µs | 421 k/s    |
| `inbound_pipeline/book_deltas`   | 1.13 µs | 884 k/s    |

## Execution pipeline (`exec.rs`)

`submit_rsa_pss` and `submit_ed25519` measure V2 limit-order construction, request
validation, sorted JSON body encoding, a realtime clock read, and request signing.
Private-key parsing stays outside the timed region. Kalshi signs
`timestamp + METHOD + path`; the body is not part of the signature. Authenticated
amend and cancel requests use that same signer.

`fill_tracked` builds one fill report and emits events for an order already
accepted in private state. `fill_external` does the same for a resting, partly
filled order that is not yet in that state. Both start at report construction
and exclude private-stream handling and HTTP recovery. Dropping the prepared
state stays outside the timed region.

| Bench                          | Median  | Throughput |
| ------------------------------ | ------- | ---------- |
| `exec_pipeline/submit_rsa_pss` | 188 µs  | 5.31 k/s   |
| `exec_pipeline/submit_ed25519` | 6.49 µs | 154 k/s    |
| `dispatch/fill_tracked`        | 3.90 µs | 257 k/s    |
| `dispatch/fill_external`       | 4.74 µs | 211 k/s    |

`fill_external` is the noisiest dispatch row. The three session estimates were
4.26 µs, 4.74 µs, and 4.95 µs. The median session's 95% confidence interval was
4.62 µs to 4.86 µs.

## Decoder rows (`micros.rs`)

A paired split in the first session showed frame decode at 754 ns of the 926 ns
quote row (81%) and 887 ns of the 1.15 µs trade row (77%). These rows isolate
that cost. The table is the three-session median, so those paired shares are not
the table ratio. Book-delta decode, 558 ns, is about half of the remeasured
1.13 µs `book_deltas` row. Those numbers come from different sessions, so the
share is not a paired split. A regression that does not move
`decode_only/book_delta` is in event application, not the decoder.

| Bench                    | Median |
| ------------------------ | ------ |
| `decode_only/ticker`     | 766 ns |
| `decode_only/trade`      | 901 ns |
| `decode_only/book_delta` | 558 ns |

Request construction stays inside the canonical submit rows. These sessions do
not isolate it.

## Allocations (`alloc.rs`)

Counts are one `bench-lto` session, not the three-session Criterion aggregate.
Quote, trade, and submit counts average 1,000 calls after 100 warmup calls. Book
and dispatch counts average 1,000 prepared calls with no separate warmup. Integer
division rounds the average down. Freed bytes are not subtracted. The counts are
not resident memory.
Book and dispatch setup and destruction stay outside the counter. Quote, trade, and
submit counts include the full timed pipeline.

| Call             | Allocations | Requested bytes |
| ---------------- | ----------- | --------------- |
| `quotes`         | 18          | 1,804           |
| `trades`         | 22          | 2,880           |
| `book_snapshot`  | 54          | 6,488           |
| `book_deltas`    | 25          | 2,620           |
| `submit_rsa_pss` | 46          | 6,998           |
| `submit_ed25519` | 45          | 6,294           |
| `fill_tracked`   | 22          | 25,944          |
| `fill_external`  | 27          | 28,488          |

## Notes

- **Quotes and trades are decode-dominated.** Parse and domain construction are
  the remainder of those rows. A regression that does not move `decode_only`
  is in the parser, not the frame decoder.
- **RSA submit is signature-bound.** These sessions do not isolate Ed25519
  signing from construction, and they do not isolate JSON key order from signing.
- **Book deltas split decoder and event application.** Decode is about half of
  `book_deltas` once fixture teardown is excluded.
- **Dispatch includes event emission.** The channel send is inside the timed
  region. `fill_external` also builds the recovered order, which is why it is
  slower and noisier than `fill_tracked`.
