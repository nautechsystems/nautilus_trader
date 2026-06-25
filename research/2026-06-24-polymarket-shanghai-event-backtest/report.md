# Polymarket 上海温度事件回测 smoke-test 报告

更新：2026-06-25

## 1. 这次验证重点

这次把 PMXT 行内 `best_bid` / `best_ask` 的语义改成 **price_change batch 后状态字段**，而不是每条扁平化 row 应用后的状态。更合理的判断是：

- `book` snapshot 和 `price_change` 才是盘口事件事实；
- PMXT 把一个 WS `price_change` message 里的多条 changes 拆成多行；
- 同一个 batch 内每行重复同一组 batch-level `best_bid` / `best_ask`；
- 因此不能逐 row 应用 delta 后立刻比较 BBO；
- 正确方式是把同一批 `price_change` rows 一次性应用，再比较 batch-level BBO。

当前更重要的校验变成两件事：

1. **price_change batch-level BBO alignment**  
   group key 使用 `timestamp_received / timestamp / market / asset_id / event_type`。同一 batch 内所有 `price_change` 一次性应用，应用完后再和 PMXT 的 `best_bid` / `best_ask` 比较。

2. **snapshot-to-snapshot replay alignment**  
   从一个 `book` snapshot 开始应用后续 `price_change`，到下一次 `book` snapshot 前，看本地 replay 出来的盘口是否能和下一次 snapshot 对齐。

3. **trade vs book sanity**  
   对每条 `last_trade_price`，看实际成交价是否能被当时本地 L2 book 解释：是否落在 bid/ask 区间内，是否按 side 打到 touch。

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
| --- | --- | --- | --- |
| `highest-temperature-in-shanghai-on-june-9-2026` | `25°C` | YES | 1.0 |
| `highest-temperature-in-shanghai-on-june-10-2026` | `28°C` | YES | 1.0 |

策略形态：

| strategy | 含义 | 主要用途 |
| --- | --- | --- |
| `maker_bbo` | 在 replay BBO 两边挂单，用 trade print 判断是否被打到 | 测 maker fill plumbing |
| `buy_hold_first_ask` | 第一次看到有效 ask 后买入并持有到结算 | 测结算/PnL sanity |
| `momentum_taker` | 每 5 分钟看 mid 变化，顺势吃单 | 测 taker 策略路径 |
| `contrarian_taker` | momentum 的反向版本 | 测反向策略路径 |

这些不是策略收益结论，只是 smoke-test。

## 4. 新的 replay quality 指标

输出聚合文件：

```text
research/2026-06-24-polymarket-shanghai-event-backtest/data/strategy_suite_summary.csv
```

关键 replay 指标如下。四个策略共享同一 event/token 的 replay quality，所以同一个 event 下四行指标相同。

| event | batch BBO mismatch | snapshot BBO mismatch | trade off-book | trade side-touch | 当前判断 |
| --- | ---: | ---: | ---: | ---: | --- |
| Jun 9 / 25°C YES | 4.62% | 24.73% | 3.96% | 63.37% | smoke-test, unvalidated |
| Jun 10 / 28°C YES | 10.41% | 20.61% | 0.97% | 75.43% | smoke-test, unvalidated |

解释：

- **batch BBO mismatch**：同一 price_change batch 一次性应用后，本地 BBO 和 PMXT batch-level `best_bid` / `best_ask` 不一致的比例。
- **snapshot BBO mismatch**：从上一个 `book` snapshot replay 到下一个 `book` snapshot 时，本地 BBO 和下一次 snapshot BBO 不一致的比例。
- **trade off-book**：成交价不在当前本地 bid/ask 区间内的比例。
- **trade side-touch**：如果 side=BUY，成交价是否等于当时 ask；如果 side=SELL，成交价是否等于当时 bid。

## 5. 现在怎么看结果

当前所有结果仍标记为：

```text
result_label = smoke_test_unvalidated
results_validated = false
```

原因已经不是“PMXT 行内 BBO 对不上”，而是：

1. batch-level BBO mismatch 虽然比逐 row 校验明显改善，但仍高于 1%；
2. snapshot-to-snapshot 对齐率还不够好；
3. replay 过程中仍出现 crossed/locked book 或 negative spread；
4. 少量 trade print 不能被当前本地 book 直接解释。

这说明第一阶段 plumbing 能跑通，但正式回测前还需要继续确认 PMXT delta 语义。

## 6. 结果表

| event | market | strategy | fills | ending inventory | gross notional | settlement PnL | label |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| Jun 9 | 25°C YES | `maker_bbo` | 288 | -71.78 | 1154.1944 | -31.2611 | smoke-test |
| Jun 9 | 25°C YES | `buy_hold_first_ask` | 1 | 10.00 | 2.2000 | 7.8000 | smoke-test |
| Jun 9 | 25°C YES | `momentum_taker` | 34 | 20.00 | 116.7600 | -11.8400 | smoke-test |
| Jun 9 | 25°C YES | `contrarian_taker` | 34 | -20.00 | 116.8500 | -3.1500 | smoke-test |
| Jun 10 | 28°C YES | `maker_bbo` | 182 | -57.9107 | 839.0407 | -53.8707 | smoke-test |
| Jun 10 | 28°C YES | `buy_hold_first_ask` | 1 | 10.00 | 2.3000 | 7.7000 | smoke-test |
| Jun 10 | 28°C YES | `momentum_taker` | 27 | 90.00 | 89.5000 | 26.5000 | smoke-test |
| Jun 10 | 28°C YES | `contrarian_taker` | 27 | -90.00 | 87.3000 | -36.7000 | smoke-test |

尤其 `buy_hold_first_ask` 选的是事后已知 winner，只是 sanity check，不是可交易策略。

## 7. 下一步

P0：

- 继续查剩余 batch BBO mismatch 样本；
- 继续查 `price_change.side` 和 `price_change.size` 的语义；
- 针对 snapshot mismatch 抽样输出 diff，看是 top-of-book 错、某些 level 漏删，还是 snapshot 本身不是完整 book；
- 检查 `timestamp` vs `timestamp_received` 排序；
- 对 trade off-book 样本做明细抽查：trade 是发生在更新前、更新后，还是 Polymarket delay / batch 造成的可解释偏差。

P1：

- 把 validation 输出扩展成可读的 sample CSV；
- 做 event-level accounting，而不是只跑单 token；
- 加 taker delay、partial fill、queue-ahead sensitivity；
- 再接 NautilusTrader catalog / custom data。
