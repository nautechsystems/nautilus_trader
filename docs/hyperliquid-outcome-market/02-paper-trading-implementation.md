# Hyperliquid 预测市场集成 - 纸面交易实现方案

## 文档信息

- **创建日期**: 2026-05-03
- **目标**: 最小化代码改造，实现预测市场纸面交易功能
- **前提**: 复用现有 WebSocket/HTTP 基础设施，专注于模拟执行逻辑

---

## 一、纸面交易 vs 实盘的关键区别

### 1.1 需要实现的内容

| 功能 | 纸面交易 | 实盘 |
|------|----------|------|
| 市场数据订阅（价格、订单簿、成交） | ✅ 需要 | ✅ 需要 |
| Instrument 定义（符号、精度、到期时间） | ✅ 需要 | ✅ 需要 |
| 订单验证（价格范围、数量限制） | ✅ 需要 | ✅ 需要 |
| 模拟撮合（基于 L2Book/Trade 数据） | ✅ 需要 | ❌ 不需要 |
| 持仓计算（二元盈亏公式） | ✅ 需要 | ✅ 需要 |
| 结算逻辑（到期后的 PnL 计算） | ✅ 需要 | ✅ 需要 |
| 真实签名和链上提交 | ❌ 不需要 | ✅ 需要 |
| Web3 钱包交互 | ❌ 不需要 | ✅ 需要 |
| 实际资金划转 | ❌ 不需要 | ✅ 需要 |

### 1.2 核心设计原则

**复用现有基础设施**：
- WebSocket 订阅方法（`subscribe_trades`, `subscribe_l2book`, `subscribe_bbo`）
- HTTP 客户端
- 消息解析逻辑
- Instrument 缓存机制

**新增模拟层**：
- 订单验证（预测市场特殊规则）
- 模拟撮合引擎
- 持仓和盈亏计算
- 结算处理

---

## 二、最小代码改造清单

### 阶段 1：基础类型定义（~2小时）

#### 2.1.1 扩展市场类型枚举

**文件**: `crates/adapters/hyperliquid/src/common/enums.rs`

```rust
// 第 47-52 行，添加 Outcome
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperliquidMarketType {
    Perp,
    Spot,
    Outcome,  // 新增：预测市场
}

// 第 951-956 行，添加 Outcome
pub enum HyperliquidProductType {
    Perp,
    Spot,
    Outcome,  // 新增
}

// 第 964-972 行，更新 symbol 解析
pub fn from_symbol(symbol: &str) -> anyhow::Result<Self> {
    if symbol.ends_with("-PERP") {
        Ok(Self::Perp)
    } else if symbol.ends_with("-SPOT") {
        Ok(Self::Spot)
    } else if symbol.ends_with("-OUTCOME") {  // 新增
        Ok(Self::Outcome)
    } else {
        anyhow::bail!("Invalid Hyperliquid symbol format: {symbol}")
    }
}
```

#### 2.1.2 定义预测市场特有数据结构

**文件**: `crates/adapters/hyperliquid/src/http/models.rs`（新增）

```rust
/// 预测市场元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeMeta {
    pub universe: Vec<OutcomeAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeAsset {
    pub name: String,                   // 如 "BTC-78K-2026-05-03"
    pub description: String,            // 市场描述
    pub sz_decimals: u32,               // 数量精度
    pub price_decimals: u32,            // 价格精度（通常6位）
    pub expiry_time: u64,               // 到期时间戳（毫秒）
    pub oracle_source: String,          // 预言机来源
    pub settlement_criteria: String,    // 结算标准描述
    pub is_expired: Option<bool>,       // 是否已到期
    pub is_settled: Option<bool>,       // 是否已结算
    pub settlement_result: Option<bool>, // 结算结果
}

/// 预测市场价格上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeAssetCtx {
    pub coin: String,
    pub mid_px: Option<String>,
    pub mark_px: Option<String>,
    pub prev_day_px: Option<String>,
    pub open_interest: Option<String>,
    pub day_ntl_vlm: Option<String>,
}
```

#### 2.1.3 定义预测市场数据类型

**文件**: `crates/adapters/hyperliquid/src/data_types.rs`（新增）

```rust
use nautilus_model::types::{Price, Quantity, Money};
use nautilus_core::UnixNanos;

/// 预测市场结算事件（用于回测/纸面交易）
#[derive(Debug, Clone)]
pub struct OutcomeSettlement {
    pub instrument_id: InstrumentId,
    pub outcome: bool,                  // true = Yes, false = No
    pub settlement_price: Price,        // 1.0 或 0.0
    pub expiry_time: UnixNanos,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl CustomDataTrait for OutcomeSettlement {
    // 实现 CustomData 接口，可在策略中监听
}

/// 预测市场验证器
pub struct OutcomeMarketValidator;

impl OutcomeMarketValidator {
    /// 验证价格范围：0.001 - 0.999
    pub fn validate_price(price: &Price) -> Result<(), String> {
        let min = Price::from("0.001");
        let max = Price::from("0.999");
        
        if price < &min || price > &max {
            return Err(format!(
                "Outcome market price {} out of range [0.001, 0.999]",
                price
            ));
        }
        Ok(())
    }
    
    /// 计算全额抵押保证金
    pub fn calculate_margin(size: Quantity, price: Price) -> Decimal {
        size.as_decimal() * price.as_decimal()
    }
}
```

---

### 阶段 2：数据解析层（~4小时）

#### 2.2.1 新增预测市场 Instrument 解析

**文件**: `crates/adapters/hyperliquid/src/http/parse.rs`

```rust
/// 解析预测市场 instrument 定义
/// 
/// 特殊之处：
/// - 价格范围：0.001 - 0.999
/// - 高精度（通常6位小数）
/// - 有明确的到期时间
pub fn parse_outcome_instruments(
    meta: &OutcomeMeta,
    asset_index_base: u32,  // 建议：200_000
) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    let mut defs = Vec::new();
    
    for (index, asset) in meta.universe.iter().enumerate() {
        let price_decimals = asset.price_decimals;
        let tick_size = Decimal::new(1, price_decimals);
        let lot_size = Decimal::new(1, asset.sz_decimals);
        
        // 符号格式：BTC-78K-2026-05-03-OUTCOME
        let symbol = format!("{}-OUTCOME", sanitize_symbol(&asset.name));
        
        let def = HyperliquidInstrumentDef {
            symbol: symbol.into(),
            raw_symbol: asset.name.clone().into(),
            base: asset.name.clone().into(),
            quote: "USDH".into(),
            market_type: HyperliquidMarketType::Outcome,
            asset_index: asset_index_base + index as u32,
            price_decimals,
            size_decimals: asset.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: Some(1),  // 无杠杆
            only_isolated: false,
            is_hip3: false,
            active: !asset.is_expired.unwrap_or(false),
            raw_data: serde_json::to_string(asset).unwrap_or_default(),
        };
        
        defs.push(def);
    }
    
    Ok(defs)
}
```

#### 2.2.2 更新 create_instrument_from_def

```rust
#[must_use]
pub fn create_instrument_from_def(
    def: &HyperliquidInstrumentDef,
    ts_init: UnixNanos,
) -> Option<InstrumentAny> {
    // ... 现有 Spot/Perp 处理 ...
    
    match def.market_type {
        HyperliquidMarketType::Outcome => {
            // 纸面交易：先映射为 CryptoPerpetual
            // 后续可考虑专门的 BinaryOption instrument
            Some(InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,  // USDH
                false,
                def.price_decimals as u8,
                def.size_decimals as u8,
                price_increment,
                size_increment,
                None, None, None, None, None, None, None, None, None, None, None, None,
                ts_init, ts_init,
            )))
        }
        // ... 其他分支 ...
    }
}
```

---

### 阶段 3：纸面交易执行层（核心）（~8小时）

#### 2.3.1 创建纸面交易模块

**目录**: `crates/adapters/hyperliquid/src/paper_trading/`

**文件**: `mod.rs`

```rust
pub mod execution;
pub mod position;
pub mod settlement;
```

**文件**: `execution.rs`

```rust
use std::collections::HashMap;
use nautilus_model::{
    orders::OrderAny,
    instruments::InstrumentAny,
    types::{Price, Quantity, Money},
    identifiers::InstrumentId,
    enums::{OrderSide, PositionSide},
};
use nautilus_core::UnixNanos;

/// 预测市场纸面交易执行器
pub struct OutcomePaperExecution {
    /// 当前 BBO（用于撮合）
    current_bbo: HashMap<InstrumentId, (Price, Price)>, // (bid, ask)
    /// 模拟持仓
    positions: HashMap<InstrumentId, OutcomePaperPosition>,
    /// 账户余额（USDH）
    balance: Decimal,
    /// 已结算市场
    settled_markets: HashMap<InstrumentId, bool>,
}

#[derive(Debug, Clone)]
pub struct OutcomePaperPosition {
    pub instrument_id: InstrumentId,
    pub entry_price: Price,
    pub size: Quantity,
    pub side: PositionSide,
    pub margin_locked: Decimal,
    pub expiry_time: UnixNanos,
}

#[derive(Debug, Clone)]
pub struct PaperFillEvent {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub size: Quantity,
    pub side: OrderSide,
    pub timestamp: UnixNanos,
}

impl OutcomePaperExecution {
    /// 提交订单（模拟）
    pub fn submit_order(
        &mut self,
        order: &OrderAny,
        instrument: &InstrumentAny,
    ) -> Result<PaperFillEvent, String> {
        // 1. 验证订单
        self.validate_order(order, instrument)?;
        
        // 2. 检查资金（全额抵押）
        let required_margin = self.calculate_required_margin(order)?;
        if required_margin > self.balance {
            return Err("Insufficient balance".to_string());
        }
        
        // 3. 模拟撮合
        let fill = self.simulate_fill(order, instrument)?;
        
        // 4. 更新持仓和余额
        self.update_position(&fill, required_margin);
        
        Ok(fill)
    }
    
    /// 验证预测市场订单
    fn validate_order(
        &self,
        order: &OrderAny,
        _instrument: &InstrumentAny,
    ) -> Result<(), String> {
        let price = order.price()
            .ok_or("Order must have price")?;
        
        // 关键验证：价格必须在 0.001-0.999 之间
        if price < Price::from("0.001") || price > Price::from("0.999") {
            return Err(format!(
                "Price {} out of range [0.001, 0.999]",
                price
            ));
        }
        
        // 验证数量 > 0
        if order.quantity().is_zero() {
            return Err("Quantity must be > 0".to_string());
        }
        
        Ok(())
    }
    
    /// 计算所需保证金（全额抵押）
    fn calculate_required_margin(&self, order: &OrderAny) -> Result<Decimal, String> {
        let price = order.price()
            .ok_or("Order must have price")?;
        let size = order.quantity();
        Ok(price.as_decimal() * size.as_decimal())
    }
    
    /// 模拟撮合（基于当前 BBO）
    fn simulate_fill(
        &self,
        order: &OrderAny,
        instrument: &InstrumentAny,
    ) -> Result<PaperFillEvent, String> {
        let instrument_id = instrument.id();
        let (bid, ask) = self.current_bbo.get(&instrument_id)
            .ok_or("No market data")?;
        
        let (fill_price, fill_size) = match order.side() {
            OrderSide::Buy => {
                let order_price = order.price().unwrap();
                if order_price >= *ask {
                    // 以 ask 成交
                    (*ask, order.quantity())
                } else {
                    return Err("Order price too low".to_string());
                }
            }
            OrderSide::Sell => {
                let order_price = order.price().unwrap();
                if order_price <= *bid {
                    // 以 bid 成交
                    (*bid, order.quantity())
                } else {
                    return Err("Order price too high".to_string());
                }
            }
            _ => return Err("Invalid side".to_string()),
        };
        
        Ok(PaperFillEvent {
            instrument_id,
            price: fill_price,
            size: fill_size,
            side: order.side(),
            timestamp: UnixNanos::now(),
        })
    }
    
    /// 更新持仓
    fn update_position(&mut self, fill: &PaperFillEvent, margin: Decimal) {
        self.balance -= margin;
        
        let position = OutcomePaperPosition {
            instrument_id: fill.instrument_id,
            entry_price: fill.price,
            size: fill.size,
            side: match fill.side {
                OrderSide::Buy => PositionSide::Long,
                OrderSide::Sell => PositionSide::Short,
                _ => PositionSide::NoPositionSide,
            },
            margin_locked: margin,
            expiry_time: UnixNanos::default(), // 从 instrument 获取
        };
        
        self.positions.insert(fill.instrument_id, position);
    }
    
    /// 更新市场数据（由 WebSocket handler 调用）
    pub fn update_bbo(&mut self, instrument_id: InstrumentId, bid: Price, ask: Price) {
        self.current_bbo.insert(instrument_id, (bid, ask));
    }
}
```

**文件**: `settlement.rs`

```rust
use rust_decimal::Decimal;
use nautilus_model::types::{Price, Money};
use nautilus_core::UnixNanos;

/// 结算结果
#[derive(Debug, Clone)]
pub struct SettlementResult {
    pub instrument_id: InstrumentId,
    pub outcome: bool,
    pub settlement_price: Price,
    pub pnl: Decimal,
    pub margin_returned: Decimal,
}

impl OutcomePaperExecution {
    /// 到期结算持仓
    pub fn settle_position(
        &mut self,
        instrument_id: InstrumentId,
        outcome: bool,
    ) -> Result<SettlementResult, String> {
        let position = self.positions.remove(&instrument_id)
            .ok_or("No position to settle")?;
        
        // 二元结算价格
        let settlement_price = if outcome {
            Price::from("1.0")
        } else {
            Price::from("0.0")
        };
        
        // 计算盈亏
        let pnl = match position.side {
            PositionSide::Long => {
                // 做多：结算价 - 入场价
                (settlement_price.as_decimal() - position.entry_price.as_decimal())
                    * position.size.as_decimal()
            }
            PositionSide::Short => {
                // 做空：入场价 - 结算价
                (position.entry_price.as_decimal() - settlement_price.as_decimal())
                    * position.size.as_decimal()
            }
            _ => Decimal::ZERO,
        };
        
        // 返还保证金 + 盈亏
        let total_return = position.margin_locked + pnl;
        self.balance += total_return;
        
        // 标记为已结算
        self.settled_markets.insert(instrument_id, outcome);
        
        Ok(SettlementResult {
            instrument_id,
            outcome,
            settlement_price,
            pnl,
            margin_returned: position.margin_locked,
        })
    }
}
```

---

### 阶段 4：WebSocket 集成（~4小时）

#### 2.4.1 更新消息处理

**文件**: `crates/adapters/hyperliquid/src/websocket/handler.rs`

```rust
// 在现有消息处理逻辑中，添加价格验证

fn handle_trade_message(&self, coin: &str, trade: &WsTradeData) -> Result<TradeTick, String> {
    let instrument = self.cache.get_by_raw_symbol(coin)?;
    
    // 如果是预测市场，验证价格范围
    if instrument.market_type == HyperliquidMarketType::Outcome {
        let price = Decimal::from_str(&trade.px)?;
        if price < dec!(0.001) || price > dec!(0.999) {
            log::warn!("Unexpected outcome price: {}", price);
        }
    }
    
    // 继续标准处理...
    parse_ws_trade_tick(trade, instrument, ts_init)
}
```

---

## 三、使用示例

### 3.1 初始化纸面交易

```rust
use nautilus_adapters::hyperliquid::paper_trading::OutcomePaperExecution;

// 创建执行器
let mut paper_exec = OutcomePaperExecution::new();
paper_exec.set_balance(dec!(10000)); // 10,000 USDH

// 订阅市场数据（复用现有 WebSocket 客户端）
ws_client.subscribe_bbo("BTC-78K-2026-05-03").await?;
ws_client.subscribe_trades("BTC-78K-2026-05-03").await?;
```

### 3.2 模拟下单

```rust
// 创建订单
let order = LimitOrder::new(
    instrument_id,
    OrderSide::Buy,
    Quantity::from("1000"),
    Price::from("0.62"),  // 62% 概率
    TimeInForce::Gtc,
    // ... 其他参数
)?;

// 提交到纸面交易引擎
match paper_exec.submit_order(&order, &instrument) {
    Ok(fill) => {
        println!("模拟成交: {} @ {}", fill.size, fill.price);
    }
    Err(e) => {
        println!("订单失败: {}", e);
    }
}
```

### 3.3 到期结算

```rust
// 到期时结算
let result = paper_exec.settle_position(
    instrument_id,
    true,  // true = Yes 赢了
)?;

println!("结算结果: PnL = {}", result.pnl);
println!("返还保证金: {}", result.margin_returned);
```

---

## 四、估计工作量

| 阶段 | 时间估计 |
|------|----------|
| 基础类型定义 | 2 小时 |
| 数据解析层 | 4 小时 |
| 纸面交易执行层（核心） | 8 小时 |
| WebSocket 集成 | 4 小时 |
| 测试和验证 | 4 小时 |
| **总计** | **~22 小时（约 3 天）** |

---

## 五、后续扩展

### 5.1 实盘交易支持

纸面交易完成后，扩展为实盘需要：
- 添加 EIP-712 签名逻辑
- 实现真实的订单提交
- 处理链上确认和错误

### 5.2 高级功能

- 多市场组合策略
- 自动到期检测和结算
- 预言机结果监听
- 历史数据回测支持

---

*文档结束*
