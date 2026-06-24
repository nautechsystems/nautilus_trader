# Polymarket 上海温度事件回测 smoke-test 报告

日期：2026-06-24

## 1. 这次做了什么

在 `nautilus_trader` 仓库下建立一个轻量研究目录，用瘦身后的 PMXT event parquet 先跑通事件级 replay 和几个 baseline 策略形态：

```text
research/2026-06-24-polymarket-shanghai-event-backtest/
├── scripts/
│   ├── run_event_backtest.py
│   └── run_strategy_suite.py
├── suite_manifest.json
├── data/
│   ├── strategy_suite_summary.csv
│   └── <event>__<market>__<side>__<strategy>__<params>/
│       ├── summary.json
│       ├── fills.csv
│       └── bbo_5min.csv
└── report.md
```

这还不是正式 NautilusTrader adapter，也还不是已验证的策略收益报告，而是 adapter 前的 smoke-test 层：先确认 PMXT 数据能被读取、replay、产出 fills / BBO / summary，同时把 replay mismatch 明确暴露出来。

## 2. 输入数据

当前脚本优先读取本仓库下：

```text
data/curated/polymarket/events/
```

如果本仓库没有复制 parquet，则临时读取本机已有：

```text
C:/Projects/PolyReaper/data/curated/polymarket/events/
```

本次跑了两个已结算事件：

| event | market | token | 结算 |
| --- | --- | --- | --- |
| `highest-temperature-in-shanghai-on-june-9-2026` | `25°C` | YES | 1.0 |
| `highest-temperature-in-shanghai-on-june-10-2026` | `28°C` | YES | 1.0 |

## 3. 策略组织方式

目前每个策略都是同一个 replay 引擎上的一个 `--strategy`：

```powershell
python research\2026-06-24-polymarket-shanghai-event-backtest\scripts\run_event_backtest.py --strategy maker_bbo
```

批量跑 suite。事件、market、side、策略和默认参数放在 `suite_manifest.json`，后续扩展优先改 manifest，不改 Python 常量：

```powershell
python research\2026-06-24-polymarket-shanghai-event-backtest\scripts\run_strategy_suite.py
```

如需显式指定数据根目录：

```powershell
python research\2026-06-24-polymarket-shanghai-event-backtest\scripts\run_strategy_suite.py --curated-root C:\path\to\events
```

建议后续继续沿用这个组织方式：

1. `run_event_backtest.py` 保持为单 event / 单 token / 单 strategy 的可复现入口；
2. `suite_manifest.json` 负责声明 event、market、side、strategy、默认参数；
3. `run_strategy_suite.py` 负责读取 manifest 并批量执行；
4. 每个 run 输出到独立参数化子目录，避免同一策略不同参数互相覆盖；
5. 每个 run 目录内包含 `summary.json`、`fills.csv`、`bbo_5min.csv`；
6. suite 聚合成 `strategy_suite_summary.csv`；
7. 后续接 NautilusTrader 时，把这些策略迁移成正式 Strategy / Actor，把现在的 replay 与 fill model 迁移成 adapter / data catalog / execution model。

## 4. 本次 baseline 策略

| strategy | 含义 | 主要假设 |
| --- | --- | --- |
| `maker_bbo` | 持续在当前 best bid / best ask 两边挂单，用 `last_trade_price` 判断是否被打到 | 保守 maker fill；不估 queue ahead |
| `buy_hold_first_ask` | 第一次看到有效 ask 时买入 10 份并持有到结算 | 用于验证结算、PnL、路径读取 |
| `momentum_taker` | 每 5 分钟看 mid 变化，涨超 0.03 买，跌超 0.03 卖 | 假设可在 BBO 顶层立即成交 |
| `contrarian_taker` | momentum 的反向版本 | 同上 |

所有策略暂不建模：

- taker delay；
- latency；
- partial fill；
- queue priority / queue ahead；
- fee / reward / rebate；
- event-level negRisk / combo 约束。

## 5. 结果摘要

注意：因为 BBO replay mismatch 仍高于 1% 阈值，当前所有结果在 JSON/CSV 中都标记为：

```text
result_label = smoke_test_unvalidated
results_validated = false
```

所以下表只能说明 pipeline 能跑、PnL 归因能落盘，不能作为策略有效性的结论。

输出聚合文件：

```text
research/2026-06-24-polymarket-shanghai-event-backtest/data/strategy_suite_summary.csv
```

每个 run 的明细输出在参数化子目录里，例如：

```text
research/2026-06-24-polymarket-shanghai-event-backtest/data/highest-temperature-in-shanghai-on-june-9-2026__25degC__yes__maker_bbo__fillconservative__q10__max100__freq5min__thr0p03/
```

| event | market | strategy | fills | ending inventory | gross notional | settlement PnL | label |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| Jun 9 | 25°C YES | `maker_bbo` | 322 | -78.93 | 1313.2632 | -37.3600 | smoke-test |
| Jun 9 | 25°C YES | `buy_hold_first_ask` | 1 | 10.00 | 2.2000 | 7.8000 | smoke-test |
| Jun 9 | 25°C YES | `momentum_taker` | 32 | 20.00 | 108.7600 | -12.8400 | smoke-test |
| Jun 9 | 25°C YES | `contrarian_taker` | 32 | -20.00 | 107.5900 | -1.7500 | smoke-test |
| Jun 10 | 28°C YES | `maker_bbo` | 202 | -62.5903 | 911.4100 | -58.1881 | smoke-test |
| Jun 10 | 28°C YES | `buy_hold_first_ask` | 1 | 10.00 | 2.3000 | 7.7000 | smoke-test |
| Jun 10 | 28°C YES | `momentum_taker` | 24 | 100.00 | 127.1000 | 35.9000 | smoke-test |
| Jun 10 | 28°C YES | `contrarian_taker` | 24 | -100.00 | 125.0000 | -45.0000 | smoke-test |

尤其 `buy_hold_first_ask` 因为选的是事后已知 winner，只是 sanity check，不是可交易策略。

## 6. Replay 质量观察

两份 event 都能完成 L2 replay，但本地重建 BBO 与 PMXT 行内 `best_bid` / `best_ask` 不完全一致。

脚本保留两个口径：

1. row-level：每条 `price_change` 后立刻对比 BBO；
2. grouped：把相同 `timestamp_received` 视为一个小批次，批量应用后对比最后一条 BBO。

grouped mismatch 明显低于 row-level mismatch，说明 PMXT `best_bid` / `best_ask` 可能更接近批次后状态，不能简单按单行事件顺序解释。但 grouped mismatch 仍然存在，并且超过当前脚本的 1% warning threshold，所以输出被标记为 `smoke_test_unvalidated`。

## 7. 下一步建议

P0：

- 固化 replay consistency check；
- 把策略 suite 的 event/market/strategy 配置挪成 YAML/JSON；
- 明确 PMXT `price_change.size`、`book` snapshot reset、`timestamp` vs `timestamp_received` 的处理规则；
- 将 single-token PnL 扩展成 event-level accounting。

P1：

- 接 NautilusTrader catalog / custom data；
- 实现 Polymarket instrument / order book / fill model adapter；
- 把 taker delay、partial fill、queue ahead 做成 sensitivity 参数；
- 加 fee / reward / rebate / settlement 数据。

P2：

- 批量跑更多 event；
- 加策略参数 sweep；
- 加报告自动生成；
- 接 live/paper 数据做 PMXT 外部数据 cross-check。
