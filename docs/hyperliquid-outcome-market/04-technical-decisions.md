# Hyperliquid 预测市场技术决策分析

## 关于 BinaryOption Instrument 的使用

### 你的观点是正确的

**应该使用 `BinaryOption` 而不是 `CryptoPerpetual`**。

### 为什么 BinaryOption 更合适

查看 `crates/model/src/instruments/binary_option.rs` 的定义：

```rust
pub struct BinaryOption {
    pub id: InstrumentId,
    pub raw_symbol: Symbol,
    pub asset_class: AssetClass,
    pub currency: Currency,
    pub activation_ns: UnixNanos,      // 合约激活时间（预测市场开始交易）
    pub expiration_ns: UnixNanos,      // 合约到期时间（预测市场结算时间）
    pub price_precision: u8,
    pub size_precision: u8,
    pub price_increment: Price,
    pub size_increment: Quantity,
    pub outcome: Option<Ustr>,         // 二元结果（Yes/No）
    pub description: Option<Ustr>,     // 市场描述
    pub max_price: Option<Price>,      // 最大价格（0.999）
    pub min_price: Option<Price>,      // 最小价格（0.001）
    pub margin_init: Decimal,
    pub margin_maint: Decimal,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub info: Option<Params>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

### BinaryOption 的优势

| 特性 | BinaryOption | CryptoPerpetual |
|------|--------------|-----------------|
| **activation_ns** | ✅ 有（市场开盘时间） | ❌ 无 |
| **expiration_ns** | ✅ 有（市场到期时间） | ❌ 无 |
| **outcome** | ✅ 有（Yes/No 结果） | ❌ 无 |
| **max/min_price** | ✅ 有（0.001-0.999） | ❌ 无 |
| **资产类别** | Alternative | Crypto |
| **语义准确性** | ✅ 专门用于二元期权 | ❌ 永续合约 |

### Polymarket 的正确实践

Polymarket adapter 在 `http/parse.rs` 中使用 `BinaryOption`：

```rust
let binary_option = BinaryOption::new_checked(
    instrument_id,
    raw_symbol,
    AssetClass::Alternative,        // 资产类别：Alternative（非传统资产）
    currency,
    activation_ns,                  // 市场开始时间
    expiration_ns,                  // 市场到期时间
    price_precision,
    6,                              // size_precision
    price_increment,
    size_increment,
    Some(def.outcome.inner()),      // "Yes" 或 "No"
    Some(Ustr::from(def.question.as_str())),  // 市场问题描述
    None,                           // max_quantity
    min_quantity,
    None,                           // max_notional
    None,                           // min_notional
    max_price,                      // Some(0.999)
    min_price,                      // Some(0.001)
    None,                           // margin_init
    None,                           // margin_maint
    def.maker_fee,
    def.taker_fee,
    Some(info),
    ts_init,
    ts_init,
)?;
```

### Hyperliquid 应该如何使用 BinaryOption

```rust
// Hyperliquid 预测市场 Instrument 创建
pub fn create_outcome_instrument(
    def: &OutcomeInstrumentDef,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let symbol = Symbol::new(&def.symbol);
    let venue = *HYPERLIQUID_VENUE;
    let instrument_id = InstrumentId::new(symbol, venue);
    let raw_symbol = Symbol::new(&def.raw_symbol);
    
    let currency = get_currency("USDH");  // 预测市场使用 USDH
    
    let price_increment = Price::from(def.tick_size.to_string());
    let size_increment = Quantity::from(def.lot_size.to_string());
    
    // 关键：使用 BinaryOption
    let binary_option = BinaryOption::new_checked(
        instrument_id,
        raw_symbol,
        AssetClass::Alternative,
        currency,
        UnixNanos::from(def.start_time * 1_000_000),  // 市场开始
        UnixNanos::from(def.expiry_time * 1_000_000), // 市场到期
        def.price_decimals as u8,
        def.size_decimals as u8,
        price_increment,
        size_increment,
        None,  // outcome - 结算时确定
        Some(Ustr::from(def.description.as_str())),
        None,  // max_quantity
        None,  // min_quantity
        None,  // max_notional
        None,  // min_notional
        Some(Price::from("0.999")),  // max_price
        Some(Price::from("0.001")),  // min_price
        Some(Decimal::ONE),          // margin_init: 100% (全额抵押)
        Some(Decimal::ONE),          // margin_maint: 100%
        Some(Decimal::ZERO),         // maker_fee: 0 (开仓免费)
        Some(def.taker_fee),         // taker_fee
        Some(build_info_json(def)),
        ts_init,
        ts_init,
    )?;
    
    Ok(InstrumentAny::BinaryOption(binary_option))
}
```

---

## 关于 Execution 模块的必要性

### 澄清：Polymarket 也有 Execution 模块

查看 `crates/adapters/polymarket/src/lib.rs`：

```rust
pub mod common;
pub mod config;
pub mod data;
pub mod execution;  // ✅ Polymarket 也有 execution
pub mod factories;
pub mod filters;
pub mod http;
pub mod providers;
pub mod signing;
pub mod websocket;
```

Polymarket 的 execution 模块非常大（71KB 的 mod.rs），包含：
- `order_builder.rs` - 订单构建
- `order_fill_tracker.rs` - 成交跟踪
- `parse.rs` - 执行报告解析
- `reconciliation.rs` - 订单对账
- `submitter.rs` - 订单提交
- `types.rs` - 执行相关类型

### 为什么 Live Adapter 需要 Execution 模块

Live Adapter（实盘适配器）必须实现 `ExecutionClient` trait：

```rust
#[async_trait(?Send)]
pub trait ExecutionClient {
    fn client_id(&self) -> ClientId;
    fn account_id(&self) -> AccountId;
    fn venue(&self) -> Option<Venue>;
    
    // 核心方法
    async fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()>;
    async fn cancel_order(&self, command: CancelOrder) -> anyhow::Result<()>;
    async fn modify_order(&self, command: ModifyOrder) -> anyhow::Result<()>;
    async fn query_order(&self, query: QueryOrder) -> anyhow::Result<()>;
    async fn query_account(&self, query: QueryAccount) -> anyhow::Result<()>;
    
    // 批量操作
    async fn submit_order_list(&self, command: SubmitOrderList) -> anyhow::Result<()>;
    async fn batch_cancel_orders(&self, command: BatchCancelOrders) -> anyhow::Result<()>;
    
    // 报告生成
    async fn generate_order_status_report(&self, query: GenerateOrderStatusReport) -> anyhow::Result<()>;
    async fn generate_order_status_reports(&self, query: GenerateOrderStatusReports) -> anyhow::Result<()>;
    async fn generate_fill_reports(&self, query: GenerateFillReports) -> anyhow::Result<()>;
    async fn generate_position_status_reports(&self, query: GeneratePositionStatusReports) -> anyhow::Result<()>;
}
```

### Execution 模块的职责

| 职责 | 说明 |
|------|------|
| **订单签名** | EIP-712 签名（Hyperliquid/Polymarket 都需要） |
| **订单提交** | HTTP POST 到交易所 API |
| **订单跟踪** | 维护 client_order_id ↔ venue_order_id 映射 |
| **成交处理** | 解析 WebSocket 成交消息，生成 FillReport |
| **状态同步** | 订单状态机管理（Accepted → PartiallyFilled → Filled） |
| **对账** | 定期同步订单和持仓状态 |
| **错误处理** | 处理拒绝、超时、网络错误 |

### Hyperliquid Execution 的特殊性

Hyperliquid 的 execution 模块在 `src/execution/mod.rs`（约 300 行），主要因为：

1. **EIP-712 签名**：Hyperliquid 需要链下签名
2. **统一账户**：永续 + 现货 + 预测市场共享保证金
3. **订单类型丰富**：支持限价、市价、条件单、追踪止损等
4. **WebSocket 双通道**：orderUpdates + userEvents

### 为什么有些 Adapter 不需要 Execution

只有 **Live Adapter（实盘适配器）** 需要 Execution：**只有 Live Adapter（实盘适配器）** 需要 Execution：

| Adapter 类型 | 示例 | 需要 Execution |
|-------------|------|---------------|
| **实盘适配器** | Hyperliquid, Polymarket, Binance, OKX | ✅ 需要 |
| **纸面交易** | 模拟执行，无需真实订单提交 | ❌ 不需要 |
| **回测数据** | 历史数据回放 | ❌ 不需要 |

### 结论

**对于纸面交易**：不需要完整的 Execution 模块，只需要：
- `OutcomePaperExecution`（模拟执行器）
- 基于 `HyperliquidDataClient` 获取市场数据
- 模拟撮合逻辑

**对于实盘交易**：需要完整的 Execution 模块，包括：
- `HyperliquidExecutionClient`（实现 ExecutionClient trait）
- EIP-712 签名
- 订单提交和状态跟踪
- 成交报告处理

---

## 修正后的开发建议

### 1. 使用 BinaryOption（推荐）

```rust
// http/parse.rs
pub fn create_outcome_instrument(
    def: &OutcomeInstrumentDef,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let binary_option = BinaryOption::new_checked(
        // ... 参数
        AssetClass::Alternative,  // 正确的资产类别
        Some(Price::from("0.999")),  // max_price
        Some(Price::from("0.001")),  // min_price
        Some(Decimal::ONE),      // 100% margin (全额抵押)
        // ... 其他参数
    )?;
    
    Ok(InstrumentAny::BinaryOption(binary_option))
}
```

### 2. 纸面交易架构（无需 Execution 模块）

```
┌─────────────────────────────────────────────────────────────┐
│  Paper Trading Strategy                                      │
└──────────────────┬──────────────────────────────────────────┘
                   │
┌──────────────────▼──────────────────────────────────────────┐
│  OutcomePaperExecution (模拟执行器)                          │
│  - 订单验证（价格范围、保证金）                               │
│  - 模拟撮合（基于 L2Book）                                   │
│  - 持仓管理                                                  │
│  - 结算计算                                                  │
└──────────────────┬──────────────────────────────────────────┘
                   │
┌──────────────────▼──────────────────────────────────────────┐
│  HyperliquidDataClient (复用)                                │
│  - WebSocket 订阅（bbo, trades, l2book）                     │
│  - Instrument 解析（BinaryOption）                           │
└─────────────────────────────────────────────────────────────┘
```

### 3. 实盘交易架构（需要 Execution 模块）

```
┌─────────────────────────────────────────────────────────────┐
│  Live Strategy                                               │
└──────────────────┬──────────────────────────────────────────┘
                   │
┌──────────────────▼──────────────────────────────────────────┐
│  HyperliquidExecutionClient (实现 ExecutionClient)           │
│  - 订单签名（EIP-712）                                       │
│  - 订单提交（HTTP POST）                                     │
│  - 状态跟踪（WebSocket）                                     │
│  - 成交处理                                                  │
└──────────────────┬──────────────────────────────────────────┘
                   │
┌──────────────────▼──────────────────────────────────────────┐
│  HyperliquidDataClient                                       │
│  - 市场数据订阅                                              │
│  - Instrument 管理                                           │
└─────────────────────────────────────────────────────────────┘
```

---

## 总结

1. **使用 BinaryOption**：这是正确的 Instrument 类型，语义准确，有专门的二元期权字段
2. **纸面交易不需要 Execution 模块**：只需要模拟执行器，复用 DataClient 获取市场数据
3. **实盘交易需要 Execution 模块**：实现 ExecutionClient trait，处理签名、提交、跟踪
4. **Polymarket 也是这个模式**：同样有 execution 模块，同样使用 BinaryOption
