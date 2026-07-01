# Polymarket L2 回测正确性测试说明

生成日期：2026-07-01

## 1. 目标

这轮先不重构 adapter / canonical schema，而是先证明现有 Polymarket L2 replay/backtest harness 的几个基础语义可被观测、可被测试、可在重构后复用。

核心问题：

1. 回放是否明确按指定时间顺序执行，避免隐式看未来；
2. PMXT flatten 后的 `price_change` batch 是否按原子批次应用；
3. `tick_size_change` 是否是因果更新，而不是提前使用未来 tick；
4. maker/taker fill、部分 trade-size fill、cash/inventory/MTM PnL 是否能和手算一致；
5. 真实 T02 received-time smoke 数据是否健康到足以作为背景样本。

## 2. 数据健康检查

脚本：

```bash
python research/2026-07-01-polymarket-backtest-correctness/scripts/check_data_health.py
```

输出：

- `research/2026-07-01-polymarket-backtest-correctness/data/data_health_summary.json`
- `research/2026-07-01-polymarket-backtest-correctness/data_health_report.md`

默认读取当前本机 `C:/Projects/PolyReaper` 下的 T02 诊断与 PMXT 小时包；如果目录迁移，可以通过 `POLYREAPER_ROOT` 或更细的 `POLYMARKET_T02_PMXT_HOURLY`、`POLYMARKET_T02_ALIGNMENT_DIAG`、`POLYMARKET_T02_RAW_ORDERING_DIAG` 覆盖。

当前结论：

| 检查项 | 当前结果 | 含义 |
| --- | ---: | --- |
| PMXT 小时包 rows | 45,508,830 | 本地 UTC 02 小时包存在 |
| T02 PMXT 可比 rows | 2,611 | 对齐窗口内 PMXT rows |
| T02 matched signatures | 2,611 / 2,611 | PMXT rows 全部能在 raw WS 中找到 |
| PMXT - raw | 0 | 没有 PMXT-only 强异常 |
| PMXT per-asset `timestamp_received` inversion | 0 | received-time replay 口径可用 |
| PMXT source `timestamp` inversion | 410 | 不能把 source timestamp 当严格有序序列 |
| raw receive inversion | 0 | 本地 raw receive order 没倒退 |
| T02 smoke rows | 8 | 8 个策略 smoke 输出存在 |
| T02 batch mismatch / tick violation | 0 / 0 | 上轮 smoke 链路稳定 |

这个检查只说明 T02 背景数据健康，不证明引擎正确。引擎正确性由 synthetic golden tests 证明。

## 3. Synthetic golden tests

测试文件：

```bash
python -m pytest research/2026-06-24-polymarket-shanghai-event-backtest/tests/test_synthetic_golden_replay.py -q
```

当前覆盖 5 个通用 case：

### 3.1 No-lookahead momentum

构造：

```text
t0 book: bid=0.50 ask=0.52
t1 batch: bid=0.56 ask=0.58
```

策略：`momentum_taker`。

手算期望：

- t0 只能记录初始 mid=0.51，不能交易；
- t1 看到 mid 从 0.51 到 0.57，才可以买；
- 第一笔 fill 必须发生在 `2026-01-01T00:00:01Z`，价格 0.58。

意义：防止策略在 t0 使用未来 t1 的 BBO。

### 3.2 Batch atomicity

构造同一个 PMXT batch key 下两行：

```text
初始 book: bid=0.50 ask=0.60
row1: BUY 0.58 size=100
row2: BUY 0.58 size=0
batch 后 book 回到 bid=0.50 ask=0.60
```

如果错误地逐 row 给策略看，row1 会产生 transient mid=0.59，触发 momentum 买入。正确行为是 batch 原子应用，最终 mid 没变，不能成交。

当前期望：fills = 0，batch mismatch = 0。

### 3.3 Replay order explicitness

构造 source timestamp 倒序、received timestamp 正序：

```text
received order: add bid 0.60 -> remove bid 0.60
source order:   remove bid 0.60 -> add bid 0.60
```

手算期望：

- `received_time` replay 最终 bid=0.50；
- `source_time` replay 最终 bid=0.60。

意义：证明回放顺序是显式参数，不是隐式混用。

### 3.4 Tick-size causality

构造：

```text
t0 book: bid=0.95 ask=0.965
t1 trade BUY 0.965，tick 仍是 0.01
t2 tick_size_change: 0.01 -> 0.001
t3 trade BUY 0.965
```

手算期望：

- t1 的 0.965 不在 0.01 tick grid 上，应计 1 次 violation；
- t3 的 0.965 在 0.001 tick grid 上，不 violation；
- fill tick checks=2，violations=1。

意义：防止提前使用未来 tick size。

### 3.5 Maker fill / partial trade-size / MTM PnL

构造：

```text
t0 book: bid=0.50 ask=0.60
t1 last_trade_price SELL 0.50 size=7
t2 final book: bid=0.55 ask=0.65
```

手算期望：

```text
成交：BUY 7 @ 0.50
cash = -3.50
inventory = 7
final mark = (0.55 + 0.65) / 2 = 0.60
MTM PnL = -3.50 + 7 * 0.60 = 0.70
```

当前测试直接断言这些数值。

## 4. 当前判断

当前 5 个 synthetic golden tests 全部通过，说明现有 harness 在这些基础语义上暂未暴露未来函数或明显记账错误：

- strategy decision 没有在初始 book 看到未来 batch；
- `price_change` batch 对策略是原子应用；
- `source_time` / `received_time` replay 顺序差异是显式且可观测的；
- `tick_size_change` 是因果更新；
- maker fill 的 trade-size cap、cash/inventory、MTM PnL 和手算一致。

## 5. 仍然没有证明的部分

这轮测试不证明：

- fee / reward / rebate；
- taker delay / cancel-before-fill；
- queue position / queue-ahead；
- settlement / final result；
- 跨 token YES/NO event-level 组合约束；
- NautilusTrader adapter 层的最终设计。

因此这轮结论应写成：

> 当前 L2 replay research harness 的基础因果顺序、batch 原子性、tick 因果更新、maker fill 和 MTM 记账已经有 synthetic golden tests 保护；可以在这些测试保护下进入 adapter/canonical schema 重构。
