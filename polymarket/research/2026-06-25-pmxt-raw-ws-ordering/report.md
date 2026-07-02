# PMXT 对齐与倒序检查报告

生成时间：2026-06-26  
研究目录：`research/2026-06-25-polymarket-raw-ws-ordering-capture/`

这份报告只回答两个问题：第一，live raw WebSocket 抓到的数据能不能和 PMXT hourly parquet 对上；第二，对上之后，raw 和 PMXT 各自有没有倒序问题。它不是回测工程契约，也不对最终责任层做定责。

## 1. 先和 PMXT 数据对一下

### 结论

能对上，而且 `T02` 样本对得很强：PMXT 里的 2611 条可比事件全部能在本地 raw WS 抓包中找到。`T01` 样本也基本能对上：494 条 PMXT 事件中 490 条匹配；剩下 4 条出现在比较窗口尾部 padding 内，raw 抓包本身已经结束，所以不适合当成 PMXT 与 raw 不一致的强证据。

### 证据

| 样本 | raw 抓包窗口 | PMXT 小时包 | raw 可比事件 | PMXT rows | matched | raw - PMXT | PMXT - raw | 判断 |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| live-10min / T01 | `2026-06-26T01:49:49.809Z`～`01:59:45.944Z` | `polymarket_orderbook_2026-06-26T01.parquet` | 514 | 494 | 490 | 24 | 4 | 基本对上；4 条 PMXT-only 在尾部 padding 附近 |
| live-until-1100 / T02 | `2026-06-26T02:25:28.635Z`～`02:59:53.643Z` | `polymarket_orderbook_2026-06-26T02.parquet` | 2699 | 2611 | 2611 | 88 | 0 | 强对齐；PMXT rows 全部能在 raw 中找到 |

这里的匹配是按行级业务签名做的，主要字段包括 `event_type`、`market`、`asset_id`、`timestamp`、`price`、`size`、`side`、`best_bid`、`best_ask` 等。它证明 PMXT 和本地 raw WS 在同一时间窗内记录的是高度重合的一批业务事件。

`raw - PMXT` 不等于 raw 错，也不等于 PMXT 漏；里面包含初始 `book`、窗口边界、PMXT 是否保存某些 live-only 事件等差异。对本次问题更关键的是 `PMXT - raw`：`T02` 为 0，说明 PMXT 的所有可比 rows 都能在 raw 里找到。

证据文件：

- `data/diagnostics/live_pmxt_alignment_diagnostics.json`
- `data/diagnostics/live_until_1100_pmxt_alignment_diagnostics.json`
- PMXT 本地文件：
  - `data/external/pmxt/polymarket/v2/orderbook/hourly/polymarket_orderbook_2026-06-26T01.parquet`
  - `data/external/pmxt/polymarket/v2/orderbook/hourly/polymarket_orderbook_2026-06-26T02.parquet`

## 2. 再检查有没有倒序问题

### 结论

需要分两层看：

1. 本地 raw WS 抓包本身没有发现同一个 `event_type + asset_id` 的 source timestamp 倒序；`local_msg_index`、`recv_monotonic_ns`、`recv_wall_time_utc` 也没有倒序。
2. PMXT parquet 在同一个 `asset_id` 内，`timestamp_received` 没有倒序，但 `timestamp` 有倒序。`T01` 一共 50 次，最大回退 374ms；`T02` 一共 410 次，最大回退 411ms。

所以现阶段最准确的说法是：PMXT 与 raw 事件内容能高度对上，但 PMXT 行内 `timestamp` 在同 asset 口径下确实会倒序；本地 raw receive order 没有复现同 asset 倒序。这个现象更指向 PMXT collector 接收顺序、flatten/parquet 口径，或者不同 WS client/edge 的投递顺序差异，但还不能唯一归因。

### raw WS 倒序检查证据

| 样本 | raw message | PING / PONG | `local_msg_index` gap / inversion | `recv_monotonic_ns` inversion | `recv_wall_time_utc` inversion | per `event_type + asset_id` timestamp inversion | cross event_type timestamp inversion |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| live-10min | 348 | 59 / 59 | 0 / 0 | 0 | 0 | 0 | 3 |
| live-until-1100 | 2341 | 268 / 268 | 0 / 0 | 0 | 0 | 0 | 8 |

解释：跨 event_type 或跨 asset 合并看会有少量 timestamp 回退，但这不等价于“同一个 selected token 倒序”。之前我们关心的是 selected token / single asset replay，所以更应该看 per `event_type + asset_id` 口径；这个口径下 raw WS 两个 live 样本都是 0。

证据文件：

- `data/diagnostics/live_raw_ws_ordering_diagnostics.json`
- `data/diagnostics/live_until_1100_raw_ws_ordering_diagnostics.json`
- `manifest.live.json`
- `manifest.live-until-1100.json`

### PMXT 倒序检查证据

| 样本 | PMXT `timestamp_received` 同 asset inversion | PMXT `timestamp` 同 asset inversion | 最大 timestamp 回退 |
| --- | ---: | ---: | ---: |
| live-10min / T01 | 0 | 50 | 374ms |
| live-until-1100 / T02 | 0 | 410 | 411ms |

按 asset 展开：

| 样本 | event_type | asset_id 简写 | timestamp inversion | 最大回退 |
| --- | --- | --- | ---: | ---: |
| T01 | price_change | `20326394...615562` | 15 | 139ms |
| T01 | price_change | `98945106...202132` | 19 | 129ms |
| T01 | price_change | `35539979...741849` | 3 | 166ms |
| T01 | price_change | `41144399...444679` | 2 | 374ms |
| T01 | price_change | `13887983...542147` | 6 | 36ms |
| T01 | price_change | `41832664...631668` | 5 | 36ms |
| T02 | price_change | `20326394...615562` | 104 | 411ms |
| T02 | price_change | `98945106...202132` | 35 | 297ms |
| T02 | price_change | `35539979...741849` | 48 | 356ms |
| T02 | price_change | `41144399...444679` | 40 | 356ms |
| T02 | price_change | `13887983...542147` | 139 | 306ms |
| T02 | price_change | `41832664...631668` | 44 | 199ms |

## 3. 最终摘要

1. PMXT 对齐结论：能对上。尤其 `T02` 样本里 PMXT 2611 条可比 rows 全部能在 raw WS 抓包中找到。
2. 倒序结论：raw WS 本地接收顺序没有同 asset 倒序；PMXT parquet 的 `timestamp_received` 没有同 asset 倒序；PMXT parquet 的 `timestamp` 有同 asset 倒序。
3. 当前不能写死的结论：不能说“Polymarket WS 已确认乱序”，也不能说“PMXT 已确认有 bug”。更稳妥的说法是：PMXT `timestamp` 口径不能被当作严格有序 replay 序列，后续是否用 `timestamp_received`、message boundary 或其他近似，需要继续讨论后再写入回测契约。

## 4. 额外检查：未来时间、晚到、延迟过大

这里按更细的风险口径重新检查了一次。定义如下：

- `receive inversion`：后一条消息的 receive time 比前一条 receive time 更早。
- `future source time`：source `timestamp` 晚于 receive time，也就是“收到了一个未来时间”。
- `delay`：`receive time - source timestamp`。
- `source before previous receive`：当前消息的 source `timestamp` 早于上一条已接收消息的 receive time。这个现象表示“这条事件发生得比较早，但现在才收到”；它不等价于同 token timestamp 倒序。

### raw WS price_change

| 样本 | receive inversion | future > 50ms | max future | delay > 1s | delay > 10s | max delay | 同组 source before previous receive > 1s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| live-10min | 0 | 0 | 2ms | 8 | 0 | 4.333s | 4，最大 4.283s |
| live-until-1100 | 0 | 0 | 19.65ms | 42 | 0 | 8.254s | 34，最大 8.252s |

解释：

- raw WS 的本地 receive clock 没有倒退。
- raw WS 有少量 source timestamp 看起来比本地 receive time 晚，但最大只有 19.65ms，且没有超过 50ms；这更像本机时钟和 Polymarket source clock 的微小偏差，不像真正“未来事件”。
- raw WS 的 `price_change` 有少量秒级晚到，最长约 8.25s，但没有超过 10s。
- 这些秒级晚到没有造成同一个 `event_type + asset_id` 的 source timestamp 倒序；也就是说它是 late delivery / burst delay，不是同 token 的事件顺序反转。

### PMXT price_change

| 样本 | 全局 row order 的 `timestamp_received` inversion | 同 asset `timestamp_received` inversion | future source time | delay > 1s | max delay | 同组 source before previous receive > 1s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| T01 | 5 | 0 | 0 | 0 | 524ms | 0 |
| T02 | 5 | 0 | 0 | 0 | 569ms | 0 |

解释：

- PMXT 的物理 row order 不是全局按 `timestamp_received` 排序的；它在 asset 分块边界会从一个 asset 的 01:59 / 02:59 跳回另一个 asset 的 01:50 / 02:25，所以全局 row order 有 5 次 receive-time 回退。
- 但在同一个 `event_type + asset_id` 内，PMXT 的 `timestamp_received` 没有倒退。
- PMXT 没有出现 source timestamp 晚于 `timestamp_received` 的情况。
- PMXT 的 `price_change` 延迟整体比本地 raw 抓包小得多，最大不到 600ms。

因此，如果讨论“接收时间是否倒退”，需要明确口径：

```text
raw WS append order：没有 receive inversion。
PMXT physical row order：不是全局 receive-time sorted。
PMXT per asset order：timestamp_received 没有倒退。
```

证据文件：

- `data/diagnostics/latency_pathology_diagnostics.json`
