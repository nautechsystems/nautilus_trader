# Network WebSocket Benchmarks

Numbers measured 2026-07-29. The tables report the median of three
back‑to‑back runs on the same host.

Refresh these numbers after a substantive WebSocket transport change or
dependency upgrade. Absolute numbers vary by machine; only same‑machine
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
- Aggregation: median of three back‑to‑back runs per table cell.

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

For policy and the general noise‑reduction recipe, see
[`BENCHMARKING.md`](../../../BENCHMARKING.md) at the repository root.

## Methodology

The benchmarks compare `tokio-tungstenite 0.30.0` and `sockudo-ws 2.0.1` in
the same binary and measurement session.

- Both use established, uncompressed streams over identical 1 MiB in‑memory
  Tokio duplex transports and a current‑thread runtime through `Sink` and `Stream`.
- Sockudo enables `simd`, `fastrand`, `tokio-runtime`, and
  `rustls-webpki-roots`; `auto_ping` and `idle_timeout` are disabled to isolate
  frame transport.
- Throughput processes 10,000 text messages per Criterion iteration.
- Round‑trip latency spans client send through echo receive.
- One‑way burst latency timestamps each binary message from a continuous
  sender and includes in‑memory queueing and receiver backpressure.
- Each p99.9 value covers 50 observations per run; it is useful but noisier
  than p50, p95, or p99.

The benchmark excludes DNS, TCP connect, TLS, HTTP upgrade, kernel network I/O,
and external network latency. It also excludes Compio, sockudo's native
split‑stream driver, compression, and keepalive traffic.

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

Lower is better. Values are microseconds and include queueing within the
in‑memory transport.

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
