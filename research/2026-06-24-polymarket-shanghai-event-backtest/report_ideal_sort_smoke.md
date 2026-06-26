# Polymarket 上海天气事件回测：理想排序 smoke report

日期：2026-06-26
范围：两个从 PMXT parquet 瘦身出来的上海最高温 Polymarket event；当前只跑最终胜出的 YES token。
状态：**research smoke test，不是生产级验证回测**。

这份报告是 Polymarket 策略研究报告格式的第一版。它不是单纯记录“程序跑通了”，而是希望每次回测都同时带上：

1. 回放契约；
2. 数据质量诊断；
3. 策略假设；
4. PnL / equity / fill 结果；
5. 明确的结果可信度标签。

## 0. 为什么上一版报告太薄

NautilusTrader 是一个很强的事件驱动交易 / 回测引擎，但它不会自动替我们生成 Polymarket 研究报告。对这个 repo 来说，我们还需要在 NautilusTrader 外面补一层 Polymarket research layer：

1. **数据适配层**：PMXT parquet -> 有序 L2 book events -> 策略输入。
2. **市场语义层**：condition / token 映射、YES/NO 结算、fee、reward、tick size、delay 规则。
3. **成交模型层**：taker / maker 假设、部分成交、队列估计。
4. **报告层**：回放质量、图表、策略假设、结果标签、已知失效风险。

这一版报告先把报告层搭起来。当前策略故意很简单；这个阶段更重要的是确定以后报告应该长什么样，而不是马上证明某个策略赚钱。

## 1. 本次回放的排序口径

当前代码对 PMXT rows 的排序是：

```text
timestamp, timestamp_received, original_row
```

含义：

- `timestamp` 暂时被当作理想的 source / event time 顺序。
- `timestamp_received` 只作为 tie-breaker，以及 price_change batch 的边界字段。
- `original_row` 用来保证在两个 timestamp 都相同时排序稳定。
- 相同 `(timestamp_received, timestamp, market, asset_id, event_type)` 的 `price_change` rows 会作为一个 batch 一次性应用，然后再和 PMXT batch-level BBO 对比。

这是第一阶段“理想历史回放研究”的合理口径。它**不是**最终 live-realistic replay contract。后面仍然要决定生产级历史回放到底应该使用 source-time、receive-time，还是带 snapshot reset / drift marker 的 hybrid replay。

## 2. 回放质量

![回放质量](report_assets/replay_quality.svg)

| Event | Market | Price-change batch BBO mismatch | Snapshot BBO mismatch | Raw snapshot BBO mismatch | Trade off-book rate |
| --- | --- | ---: | ---: | ---: | ---: |
| highest-temperature-in-shanghai-on-june-9-2026 | 25C YES | 4.13% | 14.72% | 24.41% | 3.80% |
| highest-temperature-in-shanghai-on-june-10-2026 | 28C YES | 2.91% | 8.94% | 20.49% | 0.97% |

解读：

- 作为 ideal-sort smoke test，目前已经可以用：事件能完整处理，L2 book 能维护，简单策略能端到端跑完，trade-vs-book 检查没有明显崩坏。
- 但它还不是已经验证过的生产级回测：BBO mismatch 仍然非零，尤其 snapshot 相关 mismatch 还比较明显。
- 从这些诊断看，6 月 10 日样本比 6 月 9 日更干净。
- 这些诊断必须跟随每一次策略结果一起展示；只看 PnL 很容易误判。

## 3. 市场价格路径

![BBO 和 mid 价格路径](report_assets/price_paths.svg)

这张图是 replay 后 selected YES token 的 5 分钟 BBO 采样。

每份 report 都应该有这张图，原因是：

1. 先看市场本身有没有有效移动，避免在坏数据上讨论策略。
2. 快速发现明显错误的 book，例如 crossed book、空 book、长时间 stale。
3. 在看 PnL 之前，先给 momentum / contrarian 的结果一个价格路径直觉。

## 4. 本次 smoke suite 的策略定义

| Strategy | 目的 | 当前限制 |
| --- | --- | --- |
| `buy_hold_first_ask` | 结算 sanity check：第一次能买到 winning YES 就买入并持有到结算。 | 不是可交易策略 benchmark；这份报告里使用的是事后已知的 winning token。 |
| `momentum_taker` | 简单趋势跟随 taker 规则，根据 mid-price 变化交易。 | 没有 fee、latency、market impact 模型。 |
| `contrarian_taker` | momentum 的反向规则，用作符号和路径 sanity check。 | 限制同 momentum。 |
| `maker_bbo` | maker-style quote 的 plumbing test，检查挂单 / fill / inventory / PnL 链路。 | 当前 fill model 很粗糙且偏保守，**不能**解读成真实 maker edge。 |

## 5. 策略结果

![策略 PnL](report_assets/strategy_pnl.svg)

| Event | Market | Strategy | Fills | Ending inventory | Gross notional | Settlement PnL | MTM PnL | Final mark | Return on gross notional |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Jun 9 | 25C YES | maker_bbo | 286 | -61.78 | 1134.86 | -31.44 | -31.37 | 0.999 | -2.77% |
| Jun 9 | 25C YES | buy_hold_first_ask | 1 | 10.00 | 2.20 | 7.80 | 7.79 | 0.999 | 354.55% |
| Jun 9 | 25C YES | momentum_taker | 34 | 20.00 | 129.56 | -11.84 | -11.86 | 0.999 | -9.14% |
| Jun 9 | 25C YES | contrarian_taker | 34 | -20.00 | 126.59 | -3.15 | -3.13 | 0.999 | -2.49% |
| Jun 10 | 28C YES | maker_bbo | 181 | -42.59 | 757.52 | -53.86 | -53.82 | 0.999 | -7.11% |
| Jun 10 | 28C YES | buy_hold_first_ask | 1 | 10.00 | 2.30 | 7.70 | 7.69 | 0.999 | 334.78% |
| Jun 10 | 28C YES | momentum_taker | 27 | 90.00 | 145.70 | 26.50 | 26.41 | 0.999 | 18.19% |
| Jun 10 | 28C YES | contrarian_taker | 27 | -90.00 | 143.90 | -36.70 | -36.61 | 0.999 | -25.50% |

## 6. 权益曲线

![权益曲线](report_assets/equity_curves.svg)

解读：

- `buy_hold_first_ask` 是最基本的 settlement sanity check。两个 event 都为正，因为这次选的 YES token 最终都 resolve YES。
- 6 月 10 日 momentum 为正、contrarian 为负，这和 winning YES token 在样本路径里向胜出方向移动是相符的。
- `maker_bbo` 在两个 event 都亏，但这里主要反映当前 fill model 粗糙且偏保守。它现在只是 harness / plumbing test，不是 maker 策略结论。
- 本版已修正尾部单边盘口的盯市展示：当尾部只有 `best_bid=0.999`、没有 `best_ask` 时，正库存不再用 0 作为 mark price，而是用可用 bid 做保守 mark。因此 equity 曲线尾部不会再出现由展示口径造成的假 cliff。

## 7. 当前结果可信度标签

当前 label 仍然是：

```text
smoke_test_unvalidated
```

这个标签是对的，因为下面这些还没有完全确定：

1. PMXT 历史回放契约：source-time、receive-time，还是 hybrid。
2. Snapshot 处理：什么时候 reset 本地 book，怎么标记 drift。
3. Fee / reward / rebate 模型。
4. Polymarket tick-size 和 delay rule 的历史 snapshot。
5. Maker fill model、partial fill、queue approximation。
6. Settlement / final result 元数据和 event 语义。

## 8. 后续标准 report 应该包含什么

后面每一份策略 / 因子报告都应该包含这些部分：

1. **Run manifest**：数据源、event universe、tokens、日期范围、代码 commit、参数。
2. **Replay contract**：排序规则、batch 规则、snapshot reset 规则、fill model 版本。
3. **数据质量 dashboard**：BBO mismatch、snapshot mismatch、trade-vs-book、缺失窗口、schema drift。
4. **市场概览**：价格路径、spread / depth、volume / trade count、最终结算结果。
5. **策略定义**：signal、执行规则、inventory / risk limit、fee 假设。
6. **表现 dashboard**：PnL、return on gross、equity curve、drawdown、turnover、inventory。
7. **Fill / execution 诊断**：按 side 的成交、fill price vs BBO、maker/taker split、off-book trades。
8. **敏感性检查**：不同 fill model、latency、fee、queue 假设、replay ordering。
9. **可信度标签**：smoke / research / validated / production-candidate。
10. **结论动作**：继续、修改、放弃，还是需要补数据 / infra。

## 9. 当前结论

短期研究可以先按这个 ideal-sort 模式推进：

```text
按 source timestamp 排序 -> 同 message price_change batch 分组 -> L2 replay -> 跑简单策略 -> 每个结果都携带 replay-quality diagnostics
```

这已经足够开始组织策略实验和类似因子研究的 workflow。
但它还不足以宣称生产级历史撮合精度。
