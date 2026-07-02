# PMXT 上海温度事件顺序 / 漏包风险诊断

更新：2026-06-25

## 结论先行

- **直接证据**：脚本检查 parquet schema 后，没有发现 `sequence` / `message_id` 这类字段，所以不能直接证明 WebSocket 是否漏了某条消息。
- **直接证据**：源 PMXT 小时文件列表在两个样本里都是连续的，没有发现小时级源文件缺口。
- **直接证据**：全 event parquet 不是全局严格按 `timestamp_received` 排序；但本次回测选中的 YES token 在物理顺序下 `timestamp_received` 没有倒退。
- **直接证据**：选中 token 按 `timestamp` 看存在大量倒退；也就是按收到顺序看，源侧 event timestamp 不是严格单调。
- **推断**：如果把 `timestamp` 理解为 Polymarket/PMXT 源侧服务器时间，那么 WS 到达流里确实存在源时间乱序或延迟到达；但 parquet schema 没有直接说明它一定是 Polymarket matching/server clock。
- **推断**：目前更强的证据指向“message 边界、event-time 乱序、snapshot/checkpoint 语义不完整”，还不能直接定性为 Polymarket WS 漏包。

## 指标表

| event | rows | source hours missing | selected recv inversions | selected event-time inversions | max event back | physical split keys | replay-sort split keys | batch mismatch | snapshot mismatch | trade off-book |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| june-9-2026 | 3925774 | 0 | 0 | 89346 | 17121ms | 43 | 0 | 4.13% | 14.72% | 3.80% |
| june-10-2026 | 5238231 | 0 | 0 | 95740 | 17337ms | 40 | 0 | 2.91% | 8.94% | 0.97% |

## 分项证据

### highest-temperature-in-shanghai-on-june-9-2026 / 25°C YES

**源文件 coverage**

- sequence/message-like schema columns: []
- source files: 56
- parsed source hours: 56
- first/last source hour: 2026-06-07T04:00:00+00:00 -> 2026-06-09T11:00:00+00:00
- missing source hours: []

**物理顺序**

- all rows `timestamp_received` inversions: 1176
- all rows `timestamp` inversions: 945559
- all rows max `timestamp` backstep: 3863051ms
- selected token `timestamp_received` inversions: 0
- selected token `timestamp` inversions: 89346
- selected token max `timestamp` backstep: 17121ms

**timestamp 倒退实例（selected token, physical received order）**

这些例子展示的是：下一行的 `timestamp_received` 没有倒退，但 `timestamp` 比上一行更早。

| back | recv gap | prev row | prev received | prev timestamp | prev event | current row | current received | current timestamp | current event |
| ---: | ---: | ---: | --- | --- | --- | ---: | --- | --- | --- |
| 17121ms | 0ms | 109175 | 2026-06-08T14:47:52.027000+00:00 | 2026-06-08T14:45:30.824000+00:00 | price_change BUY 0.1300 size=13.000000 | 109176 | 2026-06-08T14:47:52.027000+00:00 | 2026-06-08T14:45:13.703000+00:00 | price_change BUY 0.0100 size=169.600000 |
| 16538ms | 64ms | 109187 | 2026-06-08T14:47:52.027000+00:00 | 2026-06-08T14:45:30.857000+00:00 | price_change SELL 0.5400 size=95.000000 | 109188 | 2026-06-08T14:47:52.091000+00:00 | 2026-06-08T14:45:14.319000+00:00 | price_change SELL 0.6700 size=5.000000 |
| 16493ms | 393ms | 167534 | 2026-06-09T00:04:07.631000+00:00 | 2026-06-08T23:59:52.819000+00:00 | price_change BUY 0.3400 size=944.100000 | 167535 | 2026-06-09T00:04:08.024000+00:00 | 2026-06-08T23:59:36.326000+00:00 | price_change BUY 0.3400 size=944.100000 |

**batch / snapshot**

- price_change rows: 332608
- physical-order batch keys: 328964
- physical-order multi-row batch count / rows: 3602 / 7246
- physical-order split batch key count / rows: 43 / 86
- replay-sort split batch key count / rows: 0 / 0
- max batch size: 5
- exact duplicate price_change rows: 0
- book rows: 1275
- book rows sharing message key with price_change: 1078

**received - event timestamp lag quantiles, selected token**

```json
{
  "0.0": -19.0,
  "0.001": 2.0,
  "0.01": 24.0,
  "0.5": 1528.0,
  "0.99": 254555.0,
  "0.999": 300969.0,
  "1.0": 330692.0
}
```

**received positive gap quantiles, selected token**

```json
{
  "0.5": 317.0,
  "0.9": 2867.0,
  "0.99": 17129.74000000005,
  "0.999": 43034.65399999742,
  "1.0": 183322.0
}
```

### highest-temperature-in-shanghai-on-june-10-2026 / 28°C YES

**源文件 coverage**

- sequence/message-like schema columns: []
- source files: 56
- parsed source hours: 56
- first/last source hour: 2026-06-08T04:00:00+00:00 -> 2026-06-10T11:00:00+00:00
- missing source hours: []

**物理顺序**

- all rows `timestamp_received` inversions: 1176
- all rows `timestamp` inversions: 1250507
- all rows max `timestamp` backstep: 3835564ms
- selected token `timestamp_received` inversions: 0
- selected token `timestamp` inversions: 95740
- selected token max `timestamp` backstep: 17337ms

**timestamp 倒退实例（selected token, physical received order）**

这些例子展示的是：下一行的 `timestamp_received` 没有倒退，但 `timestamp` 比上一行更早。

| back | recv gap | prev row | prev received | prev timestamp | prev event | current row | current received | current timestamp | current event |
| ---: | ---: | ---: | --- | --- | --- | ---: | --- | --- | --- |
| 17337ms | 157ms | 353857 | 2026-06-10T09:16:38.495000+00:00 | 2026-06-10T09:16:38.433000+00:00 | price_change BUY 0.9900 size=521.930000 | 353858 | 2026-06-10T09:16:38.652000+00:00 | 2026-06-10T09:16:21.096000+00:00 | price_change BUY 0.9900 size=513.930000 |
| 16279ms | 310ms | 40954 | 2026-06-08T20:15:52.861000+00:00 | 2026-06-08T20:14:22.751000+00:00 | price_change BUY 0.2500 size=5.000000 | 40955 | 2026-06-08T20:15:53.171000+00:00 | 2026-06-08T20:14:06.472000+00:00 | book None None size=None |
| 16240ms | 440ms | 41591 | 2026-06-08T20:37:05.003000+00:00 | 2026-06-08T20:35:38.503000+00:00 | price_change SELL 0.4100 size=30.000000 | 41592 | 2026-06-08T20:37:05.443000+00:00 | 2026-06-08T20:35:22.263000+00:00 | price_change SELL 0.4100 size=15.000000 |

**batch / snapshot**

- price_change rows: 378830
- physical-order batch keys: 375253
- physical-order multi-row batch count / rows: 3553 / 7130
- physical-order split batch key count / rows: 40 / 80
- replay-sort split batch key count / rows: 0 / 0
- max batch size: 3
- exact duplicate price_change rows: 0
- book rows: 855
- book rows sharing message key with price_change: 676

**received - event timestamp lag quantiles, selected token**

```json
{
  "0.0": -17.0,
  "0.001": 15.0,
  "0.01": 30.0,
  "0.5": 231.0,
  "0.99": 207287.59999999963,
  "0.999": 266897.655,
  "1.0": 495834.0
}
```

**received positive gap quantiles, selected token**

```json
{
  "0.5": 310.0,
  "0.9": 1995.0,
  "0.99": 14731.700000000012,
  "0.999": 42238.43000000002,
  "1.0": 839125.0
}
```

## Evidence / inference / unknown

### Evidence

- schema 缺少 sequence/message id：无法用单调序列直接判定 WS 漏包。
- 全 event parquet 物理顺序存在小时级 `timestamp_received` 倒退；结合选中 token 无倒退，更像 event parquet 由多个 market/token 分块拼接，不是单 token WS 流乱序。
- 选中 token `timestamp_received` 无倒退：本次回测 token 的接收顺序本身没有乱。
- 选中 token `timestamp` 有大量倒退：按收到顺序看，源侧 event timestamp 不是严格单调，回放不能只假设 event time 完全有序。
- 同 key 多行 batch 大量存在；物理顺序有少量 split key，但当前 replay sort 后 split key 为 0。
- source hourly files 连续：没有小时级 coverage 缺口。

### Inference

- 剩余 mismatch 更可能来自 message boundary 不显式、same-message checkpoint、event-time/reception-time 语义差异、或 PMXT/Polymarket 的增量语义边界，而不是 curated 文件物理顺序写乱。
- 不能排除 WS 层漏消息；但当前 parquet 缺少能直接证明漏消息的序列字段。

### Unknown

- 原始 WebSocket message id / sequence id / hash。
- PMXT 是否在上游已经做过重连补偿或去重。
- Polymarket WS 是否对所有 channel 保证同一 market 内严格有序。
- book snapshot 是否保证 full-depth complete book，还是 checkpoint / partial view。
