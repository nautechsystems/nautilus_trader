# Polymarket 上海温度事件回测 smoke-test 报告

更新：2026-06-25

## 1. 这次验证重点

这次把 PMXT 行内 `best_bid` / `best_ask` 的语义改成 **price_change batch 后状态字段**，而不是每条扁平化 row 应用后的逐行状态。

更合理的判断口径是：

- `book` snapshot 和 `price_change` 才是盘口事件事实；
- PMXT 很可能把一个 WebSocket `price_change` message 里的多条 changes 拆成多行；
- 同一个 batch 内每行重复同一组 batch-level `best_bid` / `best_ask`；
- 因此不能逐 row 应用 delta 后立刻比较 BBO；
- 正确方式是把同一批 `price_change` rows 一次性应用，再比较 batch-level BBO。

本轮还加了两个只影响诊断、不影响本地盘口构造的处理：

1. **PMXT 边界 BBO sentinel normalization**
   诊断比较时，把 PMXT `best_ask >= 1.0` 视作“无有效 ask”，把 `best_bid <= 0.0` 视作“无有效 bid”。这只用于 BBO 对齐诊断，不会把 `1.0` 注入本地 order book。
2. **same-message snapshot 排除**
   如果 `book` snapshot 和 `price_change` 共享同一组 `(timestamp_received, timestamp, market, asset_id)`，主 snapshot-to-snapshot drift 指标会把这类 snapshot 排除，因为它们更像同一消息/同一批次里的 checkpoint，而不是独立的“下一次 snapshot”。

同时，replay 排序已调整为 `timestamp` 优先、`timestamp_received` 作为 tie-breaker。原因是 `timestamp` 更像交易所/事件时间，应驱动回放顺序；`timestamp_received` 仍保留在 batch key 中，用来表达 PMXT 批次来源。

## 2. 当前目录组织

```text
research/2026-06-24-polymarket-shanghai-event-backtest/
├── suite_manifest.json
├── scripts/
│   ├── run_event_backtest.py
│   └── run_strategy_suite.py
├── data/
│   ├── strategy_suite_summary.csv
│   └── <event>__<market>__<side>__<strategy>__<params>/
│       ├── summary.json
│       ├── fills.csv
│       └── bbo_5min.csv
└── report.md
```

批量入口：

```powershell
python research\2026-06-24-polymarket-shanghai-event-backtest\scripts\run_strategy_suite.py
```

事件、market、side、策略和默认参数放在：

```text
suite_manifest.json
```

## 3. 跑了哪些样本

| event | market | token | 结算 |
| --- | --- | --- | ---: |
| `highest-temperature-in-shanghai-on-june-9-2026` | `25°C` | YES | 1.0 |
| `highest-temperature-in-shanghai-on-june-10-2026` | `28°C` | YES | 1.0 |

策略形态：

| strategy | 含义 | 主要用途 |
| --- | --- | --- |
| `maker_bbo` | 在 replay BBO 两边挂单，用 trade print 判断是否被打到 | 测 maker fill plumbing |
| `buy_hold_first_ask` | 第一次看到有效 ask 后买入并持有到结算 | 测结算 PnL sanity |
| `momentum_taker` | 每 5 分钟看 mid 变化，顺势吃单 | 测 taker 策略路径 |
| `contrarian_taker` | momentum 的反向版本 | 测反向策略路径 |

这些不是策略收益结论，只是 smoke-test。

## 4. 新的 replay quality 指标

输出聚合文件：

```text
research/2026-06-24-polymarket-shanghai-event-backtest/data/strategy_suite_summary.csv
```

关键 replay 指标如下。四个策略共享同一 event/token 的 replay quality，所以同一个 event 下四行指标相同。

| event | batch BBO mismatch | snapshot BBO mismatch | raw snapshot BBO mismatch | trade off-book | trade side-touch | 当前判断 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Jun 9 / 25°C YES | 4.13% | 14.72% | 24.41% | 3.80% | 63.20% | smoke-test, unvalidated |
| Jun 10 / 28°C YES | 2.91% | 8.94% | 20.49% | 0.97% | 75.43% | smoke-test, unvalidated |

解释：

- **batch BBO mismatch**：同一 price_change batch 一次性应用后，本地 BBO 和 PMXT batch-level `best_bid` / `best_ask` 不一致的比例。该指标已对 PMXT 边界 sentinel 做诊断归一化。
- **snapshot BBO mismatch**：从上一个独立 `book` snapshot replay 到下一个独立 `book` snapshot 时，本地 BBO 和下一次 snapshot BBO 不一致的比例。该主指标排除了和 `price_change` 共享同一消息 key 的 snapshot。
- **raw snapshot BBO mismatch**：不排除 same-message snapshot 的原始口径，保留用于和旧结果对照。
- **trade off-book**：成交价不在当前本地 bid/ask 区间内的比例。
- **trade side-touch**：如果 side=BUY，成交价是否等于当时 ask；如果 side=SELL，成交价是否等于当时 bid。

本轮改动的效果：

- Jun10 的 batch BBO mismatch 从约 10.38% 降到 2.91%，主要来自把 PMXT `best_ask=1.0` 识别成 no-ask sentinel；
- Jun9 的 batch BBO mismatch 从约 4.22% 小幅降到 4.13%；
- Jun10 的主 snapshot mismatch 从 20.49% 降到 8.94%，Jun9 从 24.41% 降到 14.72%，说明大量 snapshot mismatch 是 same-message checkpoint 口径问题；
- raw snapshot 指标仍保留，避免把诊断口径变化误解成真实 replay 完全修复。

## 5. 现在怎么看结果

当前所有结果仍标记为：

```text
result_label = smoke_test_unvalidated
results_validated = false
```

原因是：

1. batch-level BBO mismatch 已明显改善，但仍高于 1%；
2. 独立 snapshot-to-snapshot 对齐率仍未达到正式回测阈值；
3. replay 过程中仍出现 crossed/locked book 或 negative spread；
4. 少量 trade print 不能被当前本地 book 直接解释。

这说明第一阶段 plumbing 能跑通，且主要校验口径已经更接近 PMXT 数据语义；但正式回测前还需要继续确认 PMXT delta 语义、排序语义、snapshot 完整性，以及 trade print 和盘口事件的相对时序。

## 6. 结果表

| event | market | strategy | fills | ending inventory | gross notional | settlement PnL | label |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| Jun 9 | 25°C YES | `maker_bbo` | 287 | -71.78 | 1144.8459 | -31.4462 | smoke-test |
| Jun 9 | 25°C YES | `buy_hold_first_ask` | 1 | 10.00 | 2.2000 | 7.8000 | smoke-test |
| Jun 9 | 25°C YES | `momentum_taker` | 34 | 20.00 | 129.5600 | -11.8400 | smoke-test |
| Jun 9 | 25°C YES | `contrarian_taker` | 34 | -20.00 | 126.5900 | -3.1500 | smoke-test |
| Jun 10 | 28°C YES | `maker_bbo` | 182 | -52.5903 | 767.5054 | -53.8707 | smoke-test |
| Jun 10 | 28°C YES | `buy_hold_first_ask` | 1 | 10.00 | 2.3000 | 7.7000 | smoke-test |
| Jun 10 | 28°C YES | `momentum_taker` | 27 | 90.00 | 145.7000 | 26.5000 | smoke-test |
| Jun 10 | 28°C YES | `contrarian_taker` | 27 | -90.00 | 143.9000 | -36.7000 | smoke-test |

尤其 `buy_hold_first_ask` 选的是事后已知 winner，只是 sanity check，不是可交易策略。

## 7. 下一步

P0：

- 继续查剩余 batch BBO mismatch 样本；
- 继续查 `price_change.side` 和 `price_change.size` 的精确定义；
- 针对 snapshot mismatch 输出 diff 样本，看是 top-of-book 错、某些 level 漏删，还是 snapshot 本身不是完整 book；
- 继续比较 `timestamp` vs `timestamp_received` 排序，并确认是否存在同一 WS message 被拆行后的更稳定 message id；
- 对 trade off-book 样本做明细抽查：trade 是发生在更新前、更新后，还是由 Polymarket delay / batch 时序造成的可解释偏差。

P1：

- 把 validation 输出扩展成可读的 sample CSV；
- 做 event-level accounting，而不是只跑单 token；
- 加 taker delay、partial fill、queue-ahead sensitivity；
- 再接 NautilusTrader catalog / custom data。
