# Hyperliquid Outcome Market（先做 Paper Trading）技术评审与修订计划

## 0. 结论先行

当前 `03-development-plan.md` 的方向（先从 instrument/data 入手，再到 paper trading）是对的，但核心数据模型和部分实施路径与 **当前 Hyperliquid 真实 API**、以及 **Nautilus 现有 paper trading 架构**不匹配。

如果目标是“先接入 Hyperliquid prediction market 的 paper trading”，建议改为：

1. 先修正 `outcomeMeta`/asset 编码模型（这是阻塞项）。
2. 先打通 `HyperliquidDataClient + BinaryOption instrument`。
3. 纸面执行优先复用现有 `sandbox` 模拟执行客户端，不新建独立 `outcome_paper` 撮合引擎。
4. 实盘执行（`HyperliquidExecutionClient`）相关改造延后到 paper trading 验证后。

---

## 1. 已核实事实（截至 2026-05-05）

### 1.1 Hyperliquid Outcome API 真实形态

- 实测 `POST /info {"type":"outcomeMeta"}` 返回结构为：
  - `outcomes: [{ outcome, name, description, sideSpecs[] }]`
  - `questions: [...]`
- 示例实测返回（2026-05-05）：
  - `{"outcomes":[{"outcome":2,"name":"Recurring",..."sideSpecs":[{"name":"Yes"},{"name":"No"}]}],"questions":[]}`

这与现计划文档中的 `OutcomeMeta { universe: Vec<OutcomeAsset> }` 不一致。

### 1.2 Outcome 资产编码规则（官方）

- Hyperliquid 官方 `Asset IDs` 页面已明确 Outcome：
  - `coin = "#" + asset`
  - `asset = outcome_id * 10 + side`
  - `side` 通常为 `0/1`（Yes/No）
- 实测 `allMids` 中确实出现 `#20/#21` 这样的 outcome 资产键。

### 1.3 Outcome 市场数据可直接走现有公共接口

- 实测 `l2Book` 对 `coin="#20"` 可正常返回盘口。
- 实测 `candleSnapshot` 对 `coin="#20"` 可正常返回 K 线。

结论：Outcome 数据面并不需要另起一套传输协议，关键是把 instrument 与 `coin="#<asset>"` 映射接上。

### 1.4 当前 Hyperliquid adapter 代码边界（本仓库）

- `HyperliquidMarketType` 目前仅 `Perp/Spot`，定义在：
  - `crates/adapters/hyperliquid/src/http/parse.rs`
- `HyperliquidProductType` 目前仅 `Perp/Spot`，并且 `from_symbol()` 仅识别 `-PERP/-SPOT`：
  - `crates/adapters/hyperliquid/src/common/enums.rs`
- 执行侧校验硬编码限制 `-PERP/-SPOT`：
  - `crates/adapters/hyperliquid/src/execution/mod.rs`
- 但是数据解析路径（trade/book/quote）大多是按 `instrument.raw_symbol()` + `price_precision/size_precision` 泛化处理，对 BinaryOption 兼容性较好。

### 1.5 Polymarket 的可复用经验

- Polymarket 已用 `BinaryOption` 建模（不是 perpetual 伪装）：
  - `crates/adapters/polymarket/src/http/parse.rs`
- Paper trading 场景在 Nautilus 里有现成方案：
  - 使用 `SandboxExecutionClient`（`nautilus-sandbox`）对 live data 做模拟执行。

---

## 2. 对 03 开发计划的合理性评审

## 2.1 合理部分

- “先做 paper trading，再考虑实盘”优先级正确。
- “先 instrument，再数据，再执行”分层顺序正确。
- 倾向使用 `BinaryOption` 的方向正确（与 Polymarket 实践一致）。

## 2.2 需修正的关键问题（高优先级）

1. **OutcomeMeta 数据结构假设错误（阻塞）**
- 文档假设 `universe/sz_decimals/price_decimals/expiry_time` 等字段。
- 实测并非该结构，若按现方案实现会在 Phase 1 就偏离。

2. **asset index 规则错误（阻塞）**
- 文档建议 `asset_index_base=200_000`。
- 官方规则是 `asset = outcome_id*10 + side`，且 `coin="#<asset>"`。

3. **paper trading 实现路径过重**
- 文档提议新增 `outcome_paper` 模块（独立验证/撮合/持仓/结算）。
- 这与 Nautilus 已有 `sandbox` 执行能力重叠，增加维护成本与语义分叉风险。

4. **模块落点不一致**
- 文档把 `HyperliquidMarketType` 放在 `common/enums.rs`，实际定义在 `http/parse.rs`。
- 如果按文档改，会引入额外重构与冲突。

5. **价格范围规则需降级为“可配置 + 观测”**
- `0.001~0.999` 可作为初始 guardrail，但当前应避免写死为协议真理。
- 建议用 `BinaryOption.min_price/max_price` + 风险日志，而不是强耦合协议假设。

## 2.3 中风险问题

- 执行侧（`ExecutionClient`）提早改造会牵出 account/position/report 全链路，超出 paper trading MVP。
- `HyperliquidProductType::from_symbol` 被多个账户/报表流程使用，贸然扩展会产生连锁分支。

---

## 3. 面向“先接入 paper trading”的修订实施方案

## Phase A（MVP 必做）：打通 Outcome 数据 + Instrument（1~2 天）✅ 已完成

### A1. 新增 outcomeMeta 请求与模型（按真实返回）✅

改动：

- `crates/adapters/hyperliquid/src/common/enums.rs` ✅
  - `HyperliquidInfoRequestType` 增加 `OutcomeMeta`
- `crates/adapters/hyperliquid/src/http/query.rs` ✅
  - `InfoRequest::outcome_meta()`
- `crates/adapters/hyperliquid/src/http/models.rs` ✅
  - 新增真实模型：`OutcomeMetaResponse`、`OutcomeDescriptor`、`OutcomeSideSpec`
- `crates/adapters/hyperliquid/src/http/client.rs` ✅
  - 新增 `info_outcome_meta()`

### A2. outcome instrument 生成（BinaryOption）✅

- `crates/adapters/hyperliquid/src/http/parse.rs` ✅
  - `HyperliquidMarketType` 增加 `Outcome`
  - 新增 `parse_outcome_instruments(meta)`：
    - 从 `outcome` + `sideSpecs` 生成两条 instrument（Yes/No）
    - `asset_index = outcome_id * 10 + side`
    - `raw_symbol = "#<asset_index>"`
    - `symbol` 采用稳定格式：`OUTCOME-{outcome_id}-{YES|NO}-OUTCOME`（内部规范）
  - `create_instrument_from_def` 新增 `Outcome -> InstrumentAny::BinaryOption`

### A3. 纳入现有 instrument bootstrap ✅

- `crates/adapters/hyperliquid/src/http/client.rs` ✅
  - 在 `request_instrument_defs()` 中追加 outcome defs
- 同步 asset 索引和缓存映射（`asset_indices`, `instruments_by_coin`）✅

交付结果：✅

- `HyperliquidDataClient` 能加载 outcome instruments。
- `subscribe_quotes/trades/book` 对 outcome 可直接工作（因 raw_symbol=`#xx`）。
- `request_bars` 对 outcome 可调用 `candleSnapshot`。

## Phase B（MVP 必做）：接入现有 Sandbox 纸面执行（0.5~1 天）

### B1. 不新增 `outcome_paper` 模块

直接复用：

- `SandboxExecutionClient`（`nautilus-sandbox`）
- 在 `examples/sandbox/` 新增 `hyperliquid_outcome_sandbox.py`
  - data client: Hyperliquid live data
  - exec client: sandbox simulated exec
  - strategy: 简单下单/撤单/风控例子

### B2. 风控与约束（MVP）

- 在策略层或适配层增加 outcome 下单前校验：
  - 价格范围 guardrail（默认 `0.001~0.999`，可配置）
  - 仅限支持的 order type（建议先 `LIMIT`）

交付结果：

- 完成“live data + simulated execution”的 paper trading 闭环。
- 不触碰实盘签名/链上提交流程。

## Phase C（可选）：最小执行侧兼容（仅在你需要统一下单入口时）

如果你希望 Hyperliquid exec client 在 paper 环境也复用统一命令路径，可做最小补丁：

- `execution/mod.rs` 的符号校验允许 `-OUTCOME`
- 但不启用真实 `post_action_exec`（仍由 sandbox 执行）

> 这一步不是 MVP 必需。

## Phase D（后续）：实盘 outcome execution（另开里程碑）

- 真实下单、账户状态、结算事件等放到下个阶段。
- 与 paper trading 成功解耦，降低首阶段风险。

---

## 4. 建议的任务拆分与工时（修订）

- A1/A2/A3：10~14h
- B1/B2：4~8h
- 测试与示例联调：6~10h

合计：**20~32h（约 2.5~4 天）**

相比 `03-development-plan.md` 的 40h 更聚焦 MVP，并且减少重复建设。

---

## 5. 测试策略（按风险排序）

1. API 合约测试
- `outcomeMeta` 反序列化（真实样本 fixture）
- `asset = outcome_id*10 + side` 映射测试

2. instrument 构建测试
- `BinaryOption` 字段完整性（price/size precision、min/max price、description）
- `raw_symbol="#xx"` 与 `symbol` 唯一性

3. 数据路径测试
- outcome 的 quote/trade/book 解析（复用现有 parse 函数）
- `request_bars` 对 outcome 成功

4. 端到端 sandbox 测试
- 订阅 outcome 行情
- 下单/成交/持仓与账户报告生成

---

## 6. 明确不建议（当前阶段）

- 不建议先做独立 `outcome_paper` 撮合与结算引擎。
- 不建议先做 outcome 实盘 execution 改造。
- 不建议基于未验证字段（`universe/sz_decimals/expiry_time`）编码。

---

## 7. 与现有文档的对应关系

- `03-development-plan.md` 可保留为“全量愿景版”。
- 本文（`t-plan.md`）作为“paper trading 优先落地版”。
- 建议后续在 README 标注：先执行 `t-plan`，再分支进入 execution live。

---

## 8. 参考链接

- Hyperliquid Asset IDs（包含 outcome 编码）: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/asset-ids
- Hyperliquid Info endpoint: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint
- Hyperliquid Exchange endpoint: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint
- Polymarket CLOB docs: https://docs.polymarket.com/

此外，本次评审还基于本仓库以下实现：

- `crates/adapters/hyperliquid/src/http/parse.rs`
- `crates/adapters/hyperliquid/src/http/client.rs`
- `crates/adapters/hyperliquid/src/execution/mod.rs`
- `crates/adapters/polymarket/src/http/parse.rs`
- `crates/adapters/sandbox/README.md`
