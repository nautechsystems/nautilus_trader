# Network Benchmarks

WebSocket numbers measured 2026-07-29. The tables report the median of three
back-to-back runs on the same host.

Refresh these numbers after a substantive WebSocket transport change or
dependency upgrade. Absolute numbers vary by machine; only same-machine
deltas are meaningful.

## Environment

| Item                | Value                                                                |
| ------------------- | -------------------------------------------------------------------- |
| CPU                 | AMD Ryzen Threadripper 9980X, 64 cores, 128 threads, one socket      |
| CPU topology        | SMT enabled, one NUMA node, 256 MiB L3 cache                         |
| OS                  | Ubuntu 24.04.4 LTS, `x86_64`                                         |
| Kernel              | Linux 7.0.0-28-generic                                               |
| Repository revision | `1c555c143f73bd6e66c8960561b0c46493533660` plus this benchmark patch |
| Rust                | `rustc 1.97.1`, LLVM 22.1.6                                          |
| Cargo               | `cargo 1.97.1`                                                       |
| Profile             | `bench-lto`: release, fat LTO, one codegen unit, full debug info     |

## Measurement controls

- CPU governor: `performance` on all 128 logical CPUs.
- ASLR: disabled per process with `setarch "$(uname -m)" -R`.
- CPU scheduling: SMT and boost enabled; benchmark thread not pinned.
- Latency sampling: 1,000 warmup messages, then 50,000 measured messages.
- Throughput sampling: Criterion default warmup and 100 samples.
- Aggregation: median of three back-to-back runs per table cell.

## How to reproduce

```bash
sudo cpupower frequency-set -g performance
for run in 1 2 3; do
    CARGO_BUILD_JOBS=16 setarch "$(uname -m)" -R \
        cargo bench -p nautilus-network --profile bench-lto \
        --bench websocket_transport -- --save-baseline "ws_run_$run" --noplot
    CARGO_BUILD_JOBS=16 NAUTILUS_WS_LATENCY_MESSAGES=50000 \
        setarch "$(uname -m)" -R \
        cargo bench -p nautilus-network --profile bench-lto \
        --bench websocket_latency
done
sudo cpupower frequency-set -g powersave
```

For policy and the general noise-reduction recipe, see
[`BENCHMARKING.md`](../../../BENCHMARKING.md) at the repository root.

## Methodology

The benchmarks compare `tokio-tungstenite 0.30.0` and `sockudo-ws 2.0.1` in
the same binary and measurement session.

- Both use established, uncompressed streams over identical 1 MiB in-memory
  Tokio duplex transports and a current-thread runtime through `Sink` and `Stream`.
- Sockudo enables `simd`, `fastrand`, `tokio-runtime`, and
  `rustls-webpki-roots`; `auto_ping` and `idle_timeout` are disabled to isolate
  frame transport.
- Throughput processes 10,000 text messages per Criterion iteration.
- Round-trip latency spans client send through echo receive.
- One-way burst latency timestamps each binary message from a continuous
  sender and includes in-memory queuing and receiver backpressure.
- Each p99.9 value covers 50 observations per run; it is useful but noisier
  than p50, p95, or p99.

The benchmark excludes DNS, TCP connect, TLS, HTTP upgrade, kernel network I/O,
and external network latency. It also excludes Compio, sockudo's native
split-stream driver, compression, and keepalive traffic.

## Round-trip text latency

Lower is better. Values are microseconds.

| Payload | Library                    |   p50 |   p95 |   p99 | p99.9 |
| ------: | -------------------------- | ----: | ----: | ----: | ----: |
|    64 B | `tokio-tungstenite 0.30.0` | 1.953 | 2.844 | 3.165 | 5.298 |
|    64 B | `sockudo-ws 2.0.1`         | 0.531 | 0.561 | 0.581 | 0.661 |
|   512 B | `tokio-tungstenite 0.30.0` | 2.033 | 2.985 | 3.305 | 6.149 |
|   512 B | `sockudo-ws 2.0.1`         | 0.601 | 0.631 | 0.651 | 0.721 |
| 4,096 B | `tokio-tungstenite 0.30.0` | 2.444 | 3.666 | 3.836 | 7.000 |
| 4,096 B | `sockudo-ws 2.0.1`         | 0.872 | 0.991 | 1.042 | 1.272 |

Across these payloads, `sockudo-ws 2.0.1` reduces p99 latency by 73-82%
relative to `tokio-tungstenite 0.30.0`.

## One-way binary burst latency

Lower is better. Values are microseconds and include queuing within the
in-memory transport.

| Payload | Library                    |    p50 |    p95 |    p99 | p99.9  |
| ------: | -------------------------- | -----: | -----: | -----: | -----: |
|    64 B | `tokio-tungstenite 0.30.0` |  9.645 | 11.548 | 14.072 | 21.783 |
|    64 B | `sockudo-ws 2.0.1`         |  8.994 | 10.315 | 14.012 | 21.152 |
|   512 B | `tokio-tungstenite 0.30.0` | 10.897 | 12.249 | 17.647 | 23.015 |
|   512 B | `sockudo-ws 2.0.1`         | 10.225 | 11.908 | 15.053 | 22.835 |
| 4,096 B | `tokio-tungstenite 0.30.0` | 23.897 | 28.183 | 36.356 | 50.777 |
| 4,096 B | `sockudo-ws 2.0.1`         | 19.289 | 22.654 | 27.973 | 40.031 |

At 512 B, `sockudo-ws 2.0.1` reduces p99 burst latency by 15% relative to
`tokio-tungstenite 0.30.0`.

## Text throughput

Higher is better. Values are millions of messages per second.

| Workload   | Payload | `tokio-tungstenite 0.30.0` | `sockudo-ws 2.0.1` |
| ---------- | ------: | -------------------------: | -----------------: |
| Receive    |    64 B |                      8.670 |              9.868 |
| Receive    |   512 B |                      7.187 |              8.504 |
| Receive    | 4,096 B |                      2.751 |              3.798 |
| Send       |    64 B |                      7.824 |              9.036 |
| Send       |   512 B |                      6.400 |              7.207 |
| Send       | 4,096 B |                      2.238 |              2.622 |
| Round trip |    64 B |                      0.537 |              2.084 |
| Round trip |   512 B |                      0.530 |              1.852 |
| Round trip | 4,096 B |                      0.434 |              1.156 |

At 512 B, `sockudo-ws 2.0.1` processes 18% more receives, 13% more sends, and
250% more round trips than `tokio-tungstenite 0.30.0`.

## HTTP transport comparison

Measured 2026-09-09. The comparison runs the complete Reqwest 0.13.4 client from revision
`c53a4565a112616371dbefa861c129b52fb737fd` and the direct Hyper implementation in the same binary.
The Hyper implementation uses Hyper 1.11.1, hyper-util 0.1.20, and tower-http 0.7.1, and shares
request preparation and response body reads between native and simulation execution.

### HTTP measurement controls

- Hardware: AMD Ryzen Threadripper 9980X, 64 physical cores, 128 logical CPUs.
- System: Ubuntu 24.04.4 LTS, Linux 7.0.0-28-generic, Rust 1.98.0.
- Profile: `bench-lto`, with fat LTO, one codegen unit, full debug symbols, no incremental
  compilation, and `panic = "abort"`.
- CPU governor: `performance` on all 128 policies; boost and SMT remain enabled.
- Affinity: client CPU 22 and server CPU 20, on separate physical cores sharing L3.
- ASLR: disabled per benchmark process with `setarch x86_64 -R`.
- Host activity: each attempt starts after 30 seconds without observed build activity.
  Sessions with Cargo or compiler activity observed at 250 ms intervals are rejected and retained
  separately. All nine accepted sessions have no such samples. Affinity does not isolate caches,
  memory bandwidth, interrupts, or other host activity.

The runner restores and verifies the original governor and energy-preference settings afterward.
The WebSocket measurement controls above apply only to the WebSocket results.

### HTTP workload and statistics

Both clients use an HTTP/1.1 loopback server, a current-thread client runtime, and warmed connection
pools. The workload covers GET and POST with a 256-byte POST body, response sizes of 1 KiB, 64 KiB,
and 1 MiB, and concurrency of 1 and 16. Each worker performs 32 warmup requests and 2,048 measured
requests per round, reduced to 256 for 1 MiB responses.

Five independent processes each run 12 alternating rounds per workload, producing 60 paired samples
per case and 17,756,160 timed requests across both clients. Every request checks its complete
response body, status, and selected headers. Each timed sample also checks that the pool opens no
new connections. Response checks also apply during warmup; separate resource runs enforce the same
response and timed connection-pool checks.

Throughput and p99 columns report medians of the 60 sample summaries. The p99 values are medians of
per-sample percentiles, not percentiles pooled across all requests. Paired changes report the median
of within-round Hyper/Reqwest ratios minus one. The median of paired ratios is not the ratio of two
separate medians; multimodal timings can make these differ substantially.

Approximate 95% intervals use 10,000 hierarchical bootstrap resamples, resampling the five sessions
and then the 12 paired rounds within each sampled session. They describe variation in this
experiment, not uncertainty across machines or production deployments.

### HTTP throughput

Higher is better. Positive paired changes favor Hyper.

| Response | Concurrency | Method | Reqwest req/s | Hyper req/s | Paired change | Approx. 95% interval |
| -------- | ----------- | ------ | ------------- | ----------- | ------------- | -------------------- |
| 1 KiB    | 1           | GET    | 39,912        | 42,362      | +4.6%         | [+3.8%, +6.7%]       |
| 1 KiB    | 1           | POST   | 35,654        | 37,389      | +5.0%         | [+4.2%, +6.4%]       |
| 1 KiB    | 16          | GET    | 69,009        | 69,053      | +0.2%         | [-0.1%, +1.2%]       |
| 1 KiB    | 16          | POST   | 57,570        | 56,507      | -1.4%         | [-1.8%, -0.4%]       |
| 64 KiB   | 1           | GET    | 29,516        | 30,645      | +3.6%         | [+1.5%, +4.8%]       |
| 64 KiB   | 1           | POST   | 27,054        | 28,223      | +4.1%         | [+3.0%, +6.3%]       |
| 64 KiB   | 16          | GET    | 55,870        | 55,460      | -0.5%         | [-1.1%, -0.1%]       |
| 64 KiB   | 16          | POST   | 47,779        | 47,631      | -0.3%         | [-0.6%, -0.1%]       |
| 1 MiB    | 1           | GET    | 7,682         | 7,572       | -0.5%         | [-5.8%, +4.3%]       |
| 1 MiB    | 1           | POST   | 7,758         | 7,626       | -0.9%         | [-2.9%, +2.9%]       |
| 1 MiB    | 16          | GET    | 6,294         | 6,105       | -2.6%         | [-5.3%, -0.2%]       |
| 1 MiB    | 16          | POST   | 6,392         | 6,356       | -0.3%         | [-3.4%, +1.7%]       |

Serial 1 KiB and 64 KiB throughput improves by 3.6% to 5.0%. Concurrent results range from -2.6% to
+0.2%, including small regressions. The 1 MiB serial intervals span zero, so these samples do not
establish a throughput improvement for those cases.

### HTTP latency

Lower is better. Negative paired p99 changes favor Hyper.

| Response | Concurrency | Method | Reqwest p99 (us) | Hyper p99 (us) | Paired p99 change | Approx. 95% interval |
| -------- | ----------- | ------ | ---------------- | -------------- | ----------------- | -------------------- |
| 1 KiB    | 1           | GET    | 29.0             | 27.7           | -4.5%             | [-6.4%, -3.1%]       |
| 1 KiB    | 1           | POST   | 32.5             | 31.1           | -4.7%             | [-6.3%, -3.1%]       |
| 1 KiB    | 16          | GET    | 242.8            | 241.4          | -0.1%             | [-0.5%, +0.4%]       |
| 1 KiB    | 16          | POST   | 294.2            | 400.3          | +1.8%             | [+0.2%, +5.4%]       |
| 64 KiB   | 1           | GET    | 41.1             | 41.0           | -3.1%             | [-4.5%, -0.1%]       |
| 64 KiB   | 1           | POST   | 45.7             | 42.2           | -3.4%             | [-5.8%, -1.6%]       |
| 64 KiB   | 16          | GET    | 308.3            | 305.2          | +0.4%             | [-1.5%, +1.1%]       |
| 64 KiB   | 16          | POST   | 358.1            | 360.3          | +0.2%             | [-1.4%, +0.6%]       |
| 1 MiB    | 1           | GET    | 299.7            | 251.9          | -4.1%             | [-22.1%, +12.0%]     |
| 1 MiB    | 1           | POST   | 162.5            | 164.8          | -0.2%             | [-8.9%, +4.3%]       |
| 1 MiB    | 16          | GET    | 3818.7           | 4097.1         | +5.4%             | [-1.1%, +13.4%]      |
| 1 MiB    | 16          | POST   | 3536.7           | 3741.8         | +0.4%             | [-4.0%, +10.8%]      |

The 1 KiB concurrent POST case has a paired p99 increase of 1.8%, with an approximate 95% interval
of [+0.2%, +5.4%]. Its separate p99 medians differ much more because of the timing distribution;
the paired estimate is the relevant comparison. Large-response latency remains variable across
sessions.

### HTTP resource use and limits

Four separate processes run in Reqwest, Hyper, Hyper, Reqwest order, with the same workload and
12 rounds. Resource totals include the client, server, setup, and warmup. RSS is sampled every
250 ms, so it is not an exact high-water mark or an allocation measurement.

| Backend      | Combined CPU seconds, two runs | Sampled peak RSS MiB, two runs |
| ------------ | ------------------------------ | ------------------------------ |
| Reqwest      | 80.83, 82.26                   | 55.1, 56.1                     |
| Direct Hyper | 77.87, 78.69                   | 58.3, 56.1                     |

Mean combined CPU time across the two resource runs per backend is about 4.0% lower for Hyper.
These resource figures are descriptive; two runs per backend do not establish a precise effect.

The comparison excludes TLS, HTTP/2, proxies, WAN latency, and adapter parsing. It does not establish
a production-wide speedup or an allocation improvement. The standalone comparison harness is
separate from the checked-in Criterion benchmarks; the WebSocket commands above do not reproduce
these HTTP results.

## HTTP/2 flow-control windows

Measured 2026-09-26. This comparison times response bodies from 32 KiB to 16 MiB under each HTTP/2
flow-control setting Hyper offers, and backs the `HttpClient` default of fixed 16 MiB stream and
32 MiB connection windows. The networking guide states the resulting
[flow-control policy](../../../docs/concepts/networking.md#flow-control-windows).

| Setting                                          | Stream window                   | Connection window | Growth during a transfer        |
| ------------------------------------------------ | ------------------------------- | ----------------- | ------------------------------- |
| Adaptive (`NAUTILUS_HTTP2_ADAPTIVE_WINDOW=true`) | 65,535 bytes, growing to 16 MiB | Same as stream    | PING probes and SETTINGS frames |
| Fixed 2 MiB (Hyper default)                      | 2 MiB                           | 5 MiB             | None                            |
| Fixed 8 MiB                                      | 8 MiB                           | 16 MiB            | None                            |
| Fixed 16 MiB (`HttpClient` default)              | 16 MiB                          | 32 MiB            | None                            |

### Flow-control setup

| Item      | Value                                                                             |
| --------- | --------------------------------------------------------------------------------- |
| Host      | AMD Ryzen Threadripper 9980X, Ubuntu 24.04.5 LTS, Linux 7.0.0-34-generic          |
| Toolchain | Rust 1.98.1, `release` profile                                                    |
| Libraries | Hyper 1.11.1, hyper-util 0.1.20, h2 0.4.19                                        |
| Server    | `h2` over TLS 1.3 on loopback, sending as fast as the client window allows        |
| Latency   | Relay holds each chunk for half the RTT in each direction; 1 ms timer             |
| RTT       | 20, 100, and 250 ms                                                               |
| Samples   | 20 per cell, except 10 for fixed 8 MiB                                            |
| Metric    | Median time from sending the request to holding the complete, length-checked body |

- Loopback bandwidth is effectively unlimited, so the window alone sets how many round trips a
  body needs. Real links narrow the large-body gaps.
- A new connection costs two round trips: TLS, then the request. The relay accepts TCP at once, so
  a real network adds a third round trip under every setting.
- A warm connection first serves three requests of the same size, which lets adaptive windows grow.
- Each round runs one process per setting and takes five samples per cell. Adaptive and fixed
  2 MiB ran four rounds in alternating order; fixed 8 MiB and 16 MiB ran afterwards.
- Adaptive and the final fixed 16 MiB samples ran through `HttpClient::builder`, switched by
  `NAUTILUS_HTTP2_ADAPTIVE_WINDOW`. Fixed 2 MiB leaves Hyper's window sizes unset.
- Fixed 8 MiB and the other half of the fixed 16 MiB samples came from a temporary build that set
  the sizes directly. Its fixed 16 MiB medians are within 1.5 ms of the final build's.
- Other builds shared the host. The standalone harness is not checked in.

### Flow-control results: new connections

Median milliseconds; lower is better.

| RTT (ms) | Body    | Adaptive | Fixed 2 MiB | Fixed 8 MiB | Fixed 16 MiB |
| -------- | ------- | -------- | ----------- | ----------- | ------------ |
| 20       | 32 KiB  | 45       | 45          | 46          | 45           |
| 20       | 64 KiB  | 68       | 45          | 45          | 45           |
| 20       | 256 KiB | 112      | 46          | 46          | 45           |
| 20       | 1 MiB   | 180      | 46          | 46          | 46           |
| 20       | 4 MiB   | 261      | 92          | 48          | 48           |
| 20       | 16 MiB  | 323      | 253         | 95          | 52           |
| 100      | 32 KiB  | 206      | 206         | 206         | 205          |
| 100      | 64 KiB  | 308      | 206         | 206         | 206          |
| 100      | 256 KiB | 411      | 206         | 206         | 206          |
| 100      | 1 MiB   | 616      | 207         | 207         | 207          |
| 100      | 4 MiB   | 875      | 413         | 209         | 208          |
| 100      | 16 MiB  | 1,185    | 1,037       | 416         | 211          |
| 250      | 32 KiB  | 506      | 506         | 506         | 506          |
| 250      | 64 KiB  | 758      | 506         | 506         | 506          |
| 250      | 256 KiB | 1,012    | 506         | 506         | 506          |
| 250      | 1 MiB   | 1,517    | 507         | 507         | 507          |
| 250      | 4 MiB   | 2,276    | 1,013       | 508         | 508          |
| 250      | 16 MiB  | 3,033    | 2,662       | 1,017       | 512          |

### Flow-control results: warm connections

Median milliseconds; lower is better.

| RTT (ms) | Body    | Adaptive | Fixed 2 MiB | Fixed 8 MiB | Fixed 16 MiB |
| -------- | ------- | -------- | ----------- | ----------- | ------------ |
| 20       | 32 KiB  | 22       | 22          | 22          | 22           |
| 20       | 64 KiB  | 22       | 22          | 22          | 22           |
| 20       | 256 KiB | 45       | 23          | 23          | 22           |
| 20       | 1 MiB   | 24       | 23          | 23          | 23           |
| 20       | 4 MiB   | 47       | 70          | 24          | 25           |
| 20       | 16 MiB  | 52       | 213         | 75          | 28           |
| 100      | 32 KiB  | 103      | 102         | 103         | 102          |
| 100      | 64 KiB  | 103      | 103         | 103         | 103          |
| 100      | 256 KiB | 103      | 103         | 103         | 103          |
| 100      | 1 MiB   | 154      | 103         | 104         | 103          |
| 100      | 4 MiB   | 105      | 310         | 105         | 105          |
| 100      | 16 MiB  | 212      | 935         | 315         | 108          |
| 250      | 32 KiB  | 253      | 253         | 253         | 253          |
| 250      | 64 KiB  | 253      | 253         | 253         | 253          |
| 250      | 256 KiB | 253      | 253         | 253         | 253          |
| 250      | 1 MiB   | 254      | 254         | 254         | 253          |
| 250      | 4 MiB   | 381      | 760         | 255         | 254          |
| 250      | 16 MiB  | 510      | 2,285       | 764         | 258          |

### Flow-control findings

- **Fixed 16 MiB is never slower.** In all 36 cells it matches the faster of adaptive and fixed
  2 MiB within 1 ms, or beats it.
- **Adaptive windows start small.** On a new connection, every body from 64 KiB up needs extra
  round trips. A 64 KiB body exceeds the 65,535-byte start by one byte and pays a full round trip.
- **Warm adaptive connections mostly catch up.** They match the fixed windows up to 1 MiB, except
  256 KiB at 20 ms and 1 MiB at 100 ms. They still trail fixed 16 MiB for 16 MiB bodies, and for
  4 MiB bodies at 20 ms and 250 ms.
- **Fixed 2 MiB caps large bodies.** It sends at most 2 MiB per round trip, so warm 4 MiB and
  16 MiB bodies take 1.5 to 4.5 times as long as with adaptive windows.
- **Fixed 8 MiB falls behind at 16 MiB.** It matches fixed 16 MiB up to 4 MiB, then takes two to
  three times as long.
- **Adaptive timings vary most.** Growth depends on probe timing, so 90th percentiles run up to
  253 ms above the median. Fixed 8 MiB and 16 MiB stay within 4 ms.

### Cloudflare downloads

Each run fetches the uncompressed Deribit instrument list, about 284 MB, from `api.tardis.dev` over
HTTP/2 through Cloudflare. Each uses a new client and `HttpClient::get_stream`.

| Setting      | Runs | Mid-body resets | Median completed download (s) |
| ------------ | ---- | --------------- | ----------------------------- |
| Adaptive     | 21   | 13              | 20.2                          |
| Fixed 2 MiB  | 26   | 0               | 21.1                          |
| Fixed 8 MiB  | 8    | 0               | 15.9                          |
| Fixed 16 MiB | 20   | 0               | 15.8                          |

- Adaptive runs failed after 15 to 48 MB and 2.0 to 4.5 seconds. Each ended in a connection reset
  or a peer close without TLS `close_notify`.
- A frame trace of an earlier failing run showed 17 PINGs, 12 SETTINGS frames, a window of about
  7.6 MB, and no GOAWAY.
- No fixed-window run reset, including fixed 16 MiB, which is larger than that 7.6 MB window. This
  points to the adaptive probe and SETTINGS traffic as the trigger, not the window size. Cloudflare
  does not document the rule.
- Five fixed 2 MiB runs received an HTTP 403 quota response from Tardis before any body, so 21
  completed.
- Download times depend on the Tardis origin, which varies over time. Only 12 runs alternated
  settings, six adaptive and six fixed 2 MiB, so compare times loosely.
