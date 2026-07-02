# Polymarket T02 数据健康检查

生成时间：`2026-07-02T03:32:33.581946Z`

## 结论

本地 T02 数据健康门通过，可继续用作 received-time replay smoke / synthetic golden tests 的背景样本。

注意：这个报告只检查已落地数据，不证明策略收益，也不替代 synthetic golden tests。

## PMXT 小时包 metadata

- exists: `True`
- path: `C:\Projects\PolyReaper\data\external\pmxt\polymarket\v2\orderbook\hourly\polymarket_orderbook_2026-06-26T02.parquet`
- rows: `45508830`
- row_groups: `44`
- bytes: `260120549`
- columns: `timestamp_received, timestamp, market, event_type, asset_id, bids, asks, price, size, side, best_bid, best_ask, fee_rate_bps, transaction_hash, old_tick_size, new_tick_size`

路径可通过环境变量覆盖：`POLYREAPER_ROOT`、`POLYMARKET_T02_PMXT_HOURLY`、`POLYMARKET_T02_ALIGNMENT_DIAG`、`POLYMARKET_T02_RAW_ORDERING_DIAG`。

## PMXT/raw 对齐诊断

- exists: `True`
- status: `pmxt_available_compared`
- raw_window_utc: `{'start': '2026-06-26T02:25:28.635Z', 'end': '2026-06-26T02:59:53.643Z'}`
- PMXT signatures: `2611`
- matched signatures: `2611`
- PMXT - raw: `0`
- PMXT timestamp_received inversions: `0`
- PMXT source timestamp inversions: `410`
- gate: `True`

## raw receive ordering 诊断

- exists: `True`
- parse_error_count: `0`
- local_index_inversions: `0`
- recv_monotonic_ns_inversions: `0`
- recv_wall_time_inversions: `0`
- total_asset_series_inversions: `0`
- total_event_type_series_inversions: `8`
- gate: `True`

## 本仓库 T02 smoke 输出

- exists: `True`
- path: `polymarket/research/2026-07-01-t02-smoke-pmxt/data/t02_strategy_suite_summary.csv`
- rows: `8`
- replay_orders: `['received_time']`
- result_labels: `['smoke_test_unvalidated']`
- max_batch_mismatch_rate: `0.0`
- max_fill_tick_violations: `0`
- total_fills: `3`
- gate: `True`

## 对测试设计的含义

- T02 真实样本可作为 received-time replay 的背景 sanity check。
- PMXT source timestamp 仍有倒序，因此不能用它证明 source-time replay 正确。
- 引擎正确性仍必须靠 synthetic golden tests：手工构造盘口、batch、tick、trade、PnL，并和手算结果比对。
