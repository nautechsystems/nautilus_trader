# Hyperliquid 预测市场纸面交易 - 详细开发计划

## 文档信息

- **创建日期**: 2026-05-03
- **目标**: 基于 Nautilus Trader 代码规范，完成 Hyperliquid 预测市场纸面交易功能
- **预计工作量**: ~40 小时（5 个工作日）
- **优先级**: P1（高优先级）

---

## 一、代码规范与风格分析

### 1.1 项目结构规范

基于现有 Hyperliquid adapter 分析：

```
crates/adapters/hyperliquid/src/
├── lib.rs                    # 模块导出
├── common/                   # 共享类型和工具
│   ├── enums.rs             # 枚举定义（HyperliquidMarketType 等）
│   ├── models.rs            # 数据模型（非 Nautilus 类型）
│   ├── parse.rs             # 解析工具函数
│   ├── consts.rs            # 常量定义
│   └── ...
├── config.rs                # 配置结构
├── data_types.rs            # CustomData 类型定义
├── data/                    # 数据客户端实现
│   ├── mod.rs              # DataClient trait 实现
│   └── ...
├── execution/               # 执行客户端实现
│   ├── mod.rs              # ExecutionClient trait 实现
│   └── ...
├── http/                    # HTTP API 客户端
│   ├── mod.rs
│   ├── client.rs
│   ├── models.rs           # HTTP 请求/响应模型
│   └── parse.rs            # Instrument 解析
├── websocket/              # WebSocket 客户端
│   ├── mod.rs
│   ├── client.rs
│   ├── messages.rs         # WebSocket 消息类型
│   └── parse.rs            # 消息解析
└── python/                 # Python 绑定
    └── ...
```

### 1.2 代码风格规范

**文件头模板**（所有 .rs 文件必须包含）：

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------
```

**错误处理模式**:

```rust
// 使用 anyhow::Context 添加上下文
let instruments = self
    .http_client
    .request_instruments()
    .await
    .context("failed to bootstrap instruments")?;

// 使用 anyhow::bail! 提前返回错误
anyhow::bail!("Unsupported instrument symbol format: {symbol}");
```

**日志规范**:

```rust
log::info!("Bootstrapped {} instruments", count);
log::debug!("Received WebSocket message: {:?}", msg);
log::warn!("Instrument {} not found in cache", id);
log::error!("Failed to send data: {}", e);
```

**文档注释要求**:

```rust
/// Creates a new [`HyperliquidDataClient`] instance.
///
/// # Errors
///
/// Returns an error if the HTTP client fails to initialize.
pub fn new(...) -> anyhow::Result<Self> { ... }
```

### 1.3 命名规范

| 类型 | 命名风格 | 示例 |
|------|----------|------|
| 结构体/枚举 | PascalCase | `HyperliquidMarketType`, `OutcomePaperExecution` |
| 函数/方法 | snake_case | `parse_outcome_instruments`, `validate_price` |
| 常量 | SCREAMING_SNAKE_CASE | `HYPERLIQUID_VENUE`, `MIN_OUTCOME_PRICE` |
| 模块 | snake_case | `paper_trading`, `outcome_market` |
| 文件 | snake_case | `outcome_paper_execution.rs` |

### 1.4 类型导出模式

在 `lib.rs` 中统一导出公共 API：

```rust
pub mod outcome;  // 新增预测市场模块

pub use crate::{
    outcome::{
        HyperliquidOutcomeMarket,
        OutcomePaperExecution,
        OutcomeSettlement,
    },
    // ... 现有导出
};
```

---

## 二、开发任务分解

### Phase 1: 基础类型定义（8 小时）

#### Task 1.1: 扩展 HyperliquidMarketType 枚举

**文件**: `src/common/enums.rs`

**改动内容**:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperliquidMarketType {
    Perp,
    Spot,
    Outcome,  // 新增
}
```

**测试要求**: 添加单元测试验证枚举序列化/反序列化

**验收标准**:
- [ ] `HyperliquidMarketType::Outcome` 成功添加到枚举
- [ ] JSON 序列化正确（大写 "OUTCOME"）
- [ ] `from_symbol()` 方法正确识别 `-OUTCOME` 后缀

#### Task 1.2: 扩展 HyperliquidProductType 枚举

**文件**: `src/common/enums.rs`

**改动内容**:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRefStr, EnumIter, EnumString, Serialize, Deserialize)]
pub enum HyperliquidProductType {
    #[strum(serialize = "PERP")]
    Perp,
    #[strum(serialize = "SPOT")]
    Spot,
    #[strum(serialize = "OUTCOME")]  // 新增
    Outcome,
}
```

#### Task 1.3: 定义预测市场数据结构

**文件**: `src/common/models.rs`（新增/扩展）

**新增结构**:

```rust
/// Prediction market (outcome) metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeMeta {
    pub universe: Vec<OutcomeAsset>,
}

/// Individual outcome market definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeAsset {
    pub name: String,
    pub description: String,
    pub sz_decimals: u32,
    pub price_decimals: u32,
    pub expiry_time: u64,
    pub oracle_source: String,
    pub settlement_criteria: String,
    pub is_expired: Option<bool>,
    pub is_settled: Option<bool>,
    pub settlement_result: Option<bool>,
}
```

**验收标准**:
- [ ] 结构体字段完整
- [ ] 实现必要的 trait（Debug, Clone, Serialize, Deserialize）
- [ ] 添加文档注释

#### Task 1.4: 定义 CustomData 类型

**文件**: `src/data_types.rs`（扩展）

**新增类型**:

```rust
use nautilus_model::data::custom::CustomDataTrait;

/// Outcome market settlement event for paper trading.
#[derive(Debug, Clone)]
pub struct HyperliquidOutcomeSettlement {
    pub instrument_id: InstrumentId,
    pub outcome: bool,
    pub settlement_price: Price,
    pub expiry_time: UnixNanos,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl CustomDataTrait for HyperliquidOutcomeSettlement {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn to_json(&self) -> String {
        serde_json::json!({
            "instrument_id": self.instrument_id.to_string(),
            "outcome": self.outcome,
            "settlement_price": self.settlement_price.to_string(),
            "expiry_time": self.expiry_time,
        }).to_string()
    }
    
    fn clone_box(&self) -> Box<dyn CustomDataTrait> {
        Box::new(self.clone())
    }
}
```

---

### Phase 2: 数据解析层（10 小时）

#### Task 2.1: 实现预测市场 Instrument 解析

**文件**: `src/http/parse.rs`

**新增函数**:

```rust
/// Parses outcome market instrument definitions from Hyperliquid metadata.
///
/// # Arguments
///
/// * `meta` - The outcome market metadata from Hyperliquid API.
/// * `asset_index_base` - Base offset for asset indexing (suggest 200_000).
///
/// # Errors
///
/// Returns an error if parsing fails for any asset.
pub fn parse_outcome_instruments(
    meta: &OutcomeMeta,
    asset_index_base: u32,
) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    const OUTCOME_MAX_DECIMALS: i32 = 6;
    
    let mut defs = Vec::new();
    
    for (index, asset) in meta.universe.iter().enumerate() {
        let is_expired = asset.is_expired.unwrap_or(false);
        
        let price_decimals = (OUTCOME_MAX_DECIMALS - asset.sz_decimals as i32)
            .max(0) as u32;
        let tick_size = pow10_neg(price_decimals);
        let lot_size = pow10_neg(asset.sz_decimals);
        
        // Format: BTC-78K-2026-05-03-OUTCOME
        let symbol = format!("{}-OUTCOME", sanitize_symbol(&asset.name));
        
        let def = HyperliquidInstrumentDef {
            symbol: symbol.into(),
            raw_symbol: asset.name.as_str().into(),
            base: asset.name.clone().into(),
            quote: "USDH".into(),
            market_type: HyperliquidMarketType::Outcome,
            asset_index: asset_index_base + index as u32,
            price_decimals,
            size_decimals: asset.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: Some(1),  // No leverage
            only_isolated: false,
            is_hip3: false,
            active: !is_expired,
            raw_data: serde_json::to_string(asset).unwrap_or_default(),
        };
        
        defs.push(def);
    }
    
    Ok(defs)
}
```

**测试要求**:

```rust
#[cfg(test)]
mod outcome_tests {
    use super::*;
    
    #[rstest]
    fn test_parse_outcome_instruments() {
        let meta = OutcomeMeta {
            universe: vec![
                OutcomeAsset {
                    name: "BTC-78K-2026-05-03".to_string(),
                    description: "Will BTC be above $78,213 on May 3?".to_string(),
                    sz_decimals: 2,
                    price_decimals: 4,
                    expiry_time: 1746295800000,
                    oracle_source: "pyth".to_string(),
                    settlement_criteria: "BTC price at 11:30 AM UTC".to_string(),
                    is_expired: Some(false),
                    is_settled: Some(false),
                    settlement_result: None,
                },
            ],
        };
        
        let defs = parse_outcome_instruments(&meta, 200_000).unwrap();
        
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].symbol, "BTC-78K-2026-05-03-OUTCOME");
        assert_eq!(defs[0].market_type, HyperliquidMarketType::Outcome);
        assert_eq!(defs[0].max_leverage, Some(1));
        assert!(defs[0].active);
    }
    
    #[rstest]
    fn test_outcome_price_range_validation() {
        // Test price range 0.001 - 0.999
        let valid_prices = vec!["0.001", "0.500", "0.999"];
        let invalid_prices = vec!["0.000", "1.000", "1.500"];
        
        for price in valid_prices {
            assert!(OutcomeMarketValidator::validate_price(&Price::from(price)).is_ok());
        }
        
        for price in invalid_prices {
            assert!(OutcomeMarketValidator::validate_price(&Price::from(price)).is_err());
        }
    }
}
```

#### Task 2.2: 更新 Instrument 创建逻辑

**文件**: `src/http/parse.rs`

**修改函数**: `create_instrument_from_def`

```rust
match def.market_type {
    HyperliquidMarketType::Spot => { /* existing */ }
    HyperliquidMarketType::Perp => { /* existing */ }
    HyperliquidMarketType::Outcome => {
        // For paper trading, map to CryptoPerpetual initially
        // Consider creating dedicated BinaryOption instrument later
        let settlement_currency = get_currency("USDH");
        
        Some(InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            settlement_currency,
            false,  // not_inverse
            def.price_decimals as u8,
            def.size_decimals as u8,
            price_increment,
            size_increment,
            None, None, None, None, None, None, None, None, None, None, None, None,
            ts_init, ts_init,
        )))
    }
}
```

---

### Phase 3: 纸面交易核心（16 小时）

#### Task 3.1: 创建纸面交易模块结构

**目录**: `src/outcome_paper/`（新增）

**文件**: `src/outcome_paper/mod.rs`

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Paper trading support for Hyperliquid outcome (prediction) markets.
//!
//! This module provides simulation capabilities for testing strategies
//! on prediction markets without real capital at risk.

pub mod execution;
pub mod position;
pub mod settlement;
pub mod validation;

pub use execution::{
    OutcomePaperExecution,
    OutcomePaperPosition,
    PaperFillEvent,
    PaperOrderResult,
};

pub use position::PositionTracker;
pub use settlement::SettlementEngine;
pub use validation::OutcomeOrderValidator;
```

#### Task 3.2: 实现订单验证器

**文件**: `src/outcome_paper/validation.rs`

```rust
//! Order validation for outcome markets.

use nautilus_model::{
    orders::OrderAny,
    types::Price,
};

/// Validates orders for outcome markets.
pub struct OutcomeOrderValidator;

impl OutcomeOrderValidator {
    /// Minimum valid outcome price (0.1% probability).
    pub const MIN_PRICE: &str = "0.001";
    /// Maximum valid outcome price (99.9% probability).
    pub const MAX_PRICE: &str = "0.999";
    
    /// Validates an order for outcome market constraints.
    ///
    /// # Checks
    ///
    /// 1. Price must be within [0.001, 0.999]
    /// 2. Quantity must be positive
    /// 3. Order type must be Limit (no market orders for prediction markets)
    ///
    /// # Errors
    ///
    /// Returns an error string if validation fails.
    pub fn validate(order: &OrderAny) -> Result<(), String> {
        // Validate price exists and is in range
        let price = order.price()
            .ok_or("Outcome orders require a price")?;
        
        let min = Price::from(Self::MIN_PRICE);
        let max = Price::from(Self::MAX_PRICE);
        
        if price < &min || price > &max {
            return Err(format!(
                "Outcome price {} must be in range [{}, {}]",
                price, Self::MIN_PRICE, Self::MAX_PRICE
            ));
        }
        
        // Validate quantity
        if order.quantity().is_zero() {
            return Err("Quantity must be greater than zero".to_string());
        }
        
        // Validate order type
        match order.order_type() {
            nautilus_model::enums::OrderType::Limit => Ok(()),
            _ => Err(format!(
                "Outcome markets only support Limit orders, got {:?}",
                order.order_type()
            )),
        }
    }
    
    /// Calculates required margin for full collateral.
    pub fn calculate_margin(order: &OrderAny) -> Option<rust_decimal::Decimal> {
        let price = order.price()?;
        let size = order.quantity();
        Some(price.as_decimal() * size.as_decimal())
    }
}
```

#### Task 3.3: 实现纸面交易执行器

**文件**: `src/outcome_paper/execution.rs`

```rust
//! Paper trading execution engine for outcome markets.

use std::collections::HashMap;
use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::InstrumentAny,
    orders::OrderAny,
    types::{Price, Quantity},
    enums::{OrderSide, PositionSide, OrderStatus},
    reports::FillReport,
};
use rust_decimal::Decimal;

use crate::outcome_paper::validation::OutcomeOrderValidator;

/// Simulated position for paper trading.
#[derive(Debug, Clone)]
pub struct OutcomePaperPosition {
    pub instrument_id: InstrumentId,
    pub entry_price: Price,
    pub size: Quantity,
    pub side: PositionSide,
    pub margin_locked: Decimal,
    pub entry_time: UnixNanos,
    pub expiry_time: UnixNanos,
}

/// Simulated fill event.
#[derive(Debug, Clone)]
pub struct PaperFillEvent {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub size: Quantity,
    pub side: OrderSide,
    pub timestamp: UnixNanos,
    pub trade_id: String,
}

/// Order submission result.
#[derive(Debug, Clone)]
pub struct PaperOrderResult {
    pub status: OrderStatus,
    pub fills: Vec<PaperFillEvent>,
    pub margin_used: Decimal,
    pub message: Option<String>,
}

/// Paper trading execution engine for outcome markets.
#[derive(Debug)]
pub struct OutcomePaperExecution {
    /// Current best bid/offer for each instrument.
    current_bbo: AHashMap<InstrumentId, (Price, Price)>,
    /// Active positions.
    positions: AHashMap<InstrumentId, OutcomePaperPosition>,
    /// Account balance in USDH.
    balance: Decimal,
    /// Settled markets and their outcomes.
    settled_markets: AHashMap<InstrumentId, bool>,
    /// Order history for tracking.
    order_history: Vec<(OrderAny, PaperOrderResult)>,
}

impl OutcomePaperExecution {
    /// Creates a new paper trading instance with initial balance.
    pub fn new(initial_balance: Decimal) -> Self {
        Self {
            current_bbo: AHashMap::new(),
            positions: AHashMap::new(),
            balance: initial_balance,
            settled_markets: AHashMap::new(),
            order_history: Vec::new(),
        }
    }
    
    /// Returns the current balance.
    #[must_use]
    pub fn balance(&self) -> Decimal {
        self.balance
    }
    
    /// Returns a reference to the position for the given instrument.
    #[must_use]
    pub fn position(&self, instrument_id: InstrumentId) -> Option<&OutcomePaperPosition> {
        self.positions.get(&instrument_id)
    }
    
    /// Submits an order for paper trading simulation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Order validation fails
    /// - Insufficient balance for margin
    /// - No market data available for the instrument
    pub fn submit_order(
        &mut self,
        order: &OrderAny,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<PaperOrderResult> {
        // Validate order
        OutcomeOrderValidator::validate(order)
            .map_err(|e| anyhow::anyhow!("Order validation failed: {e}"))?;
        
        // Check balance
        let required_margin = OutcomeOrderValidator::calculate_margin(order)
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate margin"))?;
        
        if required_margin > self.balance {
            anyhow::bail!(
                "Insufficient balance: required {}, available {}",
                required_margin, self.balance
            );
        }
        
        // Simulate fill
        let fill = self.simulate_fill(order, instrument)?;
        
        // Update state
        self.balance -= required_margin;
        self.update_or_create_position(&fill, required_margin);
        
        let result = PaperOrderResult {
            status: OrderStatus::Filled,
            fills: vec![fill],
            margin_used: required_margin,
            message: None,
        };
        
        self.order_history.push((order.clone(), result.clone()));
        
        Ok(result)
    }
    
    /// Updates market data from WebSocket.
    pub fn update_bbo(&mut self, instrument_id: InstrumentId, bid: Price, ask: Price) {
        self.current_bbo.insert(instrument_id, (bid, ask));
    }
    
    fn simulate_fill(
        &self,
        order: &OrderAny,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<PaperFillEvent> {
        let instrument_id = instrument.id();
        let (bid, ask) = self.current_bbo.get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("No market data for {}", instrument_id))?;
        
        let (fill_price, side) = match order.side() {
            OrderSide::Buy => {
                let order_price = order.price().unwrap();
                if order_price < *ask {
                    anyhow::bail!(
                        "Buy order price {} below ask {}",
                        order_price, ask
                    );
                }
                (*ask, OrderSide::Buy)
            }
            OrderSide::Sell => {
                let order_price = order.price().unwrap();
                if order_price > *bid {
                    anyhow::bail!(
                        "Sell order price {} above bid {}",
                        order_price, bid
                    );
                }
                (*bid, OrderSide::Sell)
            }
            _ => anyhow::bail!("Invalid order side"),
        };
        
        Ok(PaperFillEvent {
            instrument_id,
            price: fill_price,
            size: order.quantity(),
            side,
            timestamp: UnixNanos::now(),
            trade_id: format!("paper-{}", uuid::Uuid::new_v4()),
        })
    }
    
    fn update_or_create_position(&mut self, fill: &PaperFillEvent, margin: Decimal) {
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
            entry_time: fill.timestamp,
            expiry_time: UnixNanos::default(), // TODO: Get from instrument
        };
        
        self.positions.insert(fill.instrument_id, position);
    }
}
```

#### Task 3.4: 实现结算引擎

**文件**: `src/outcome_paper/settlement.rs`

```rust
//! Settlement engine for outcome markets.

use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::InstrumentId,
    types::{Price, Money},
};
use rust_decimal::Decimal;

use crate::outcome_paper::execution::OutcomePaperPosition;

/// Settlement result for a position.
#[derive(Debug, Clone)]
pub struct SettlementResult {
    pub instrument_id: InstrumentId,
    pub outcome: bool,
    pub settlement_price: Price,
    pub pnl: Decimal,
    pub margin_returned: Decimal,
    pub timestamp: UnixNanos,
}

/// Engine for settling outcome market positions.
pub struct SettlementEngine;

impl SettlementEngine {
    /// Settles a position based on the final outcome.
    ///
    /// # Arguments
    ///
    /// * `position` - The position to settle.
    /// * `outcome` - `true` for Yes, `false` for No.
    ///
    /// # Returns
    ///
    /// The settlement result including PnL.
    #[must_use]
    pub fn settle_position(
        position: &OutcomePaperPosition,
        outcome: bool,
    ) -> SettlementResult {
        // Binary settlement: 1.0 for Yes, 0.0 for No
        let settlement_price = if outcome {
            Price::from("1.0")
        } else {
            Price::from("0.0")
        };
        
        // Calculate PnL
        let pnl = match position.side {
            nautilus_model::enums::PositionSide::Long => {
                // Long: (settlement - entry) * size
                (settlement_price.as_decimal() - position.entry_price.as_decimal())
                    * position.size.as_decimal()
            }
            nautilus_model::enums::PositionSide::Short => {
                // Short: (entry - settlement) * size
                (position.entry_price.as_decimal() - settlement_price.as_decimal())
                    * position.size.as_decimal()
            }
            _ => Decimal::ZERO,
        };
        
        SettlementResult {
            instrument_id: position.instrument_id,
            outcome,
            settlement_price,
            pnl,
            margin_returned: position.margin_locked,
            timestamp: UnixNanos::now(),
        }
    }
    
    /// Calculates potential PnL for a position at a given price.
    #[must_use]
    pub fn calculate_unrealized_pnl(
        position: &OutcomePaperPosition,
        current_price: Price,
    ) -> Decimal {
        match position.side {
            nautilus_model::enums::PositionSide::Long => {
                (current_price.as_decimal() - position.entry_price.as_decimal())
                    * position.size.as_decimal()
            }
            nautilus_model::enums::PositionSide::Short => {
                (position.entry_price.as_decimal() - current_price.as_decimal())
                    * position.size.as_decimal()
            }
            _ => Decimal::ZERO,
        }
    }
}
```

---

### Phase 4: WebSocket 集成（6 小时）

#### Task 4.1: 更新 WebSocket 消息处理

**文件**: `src/websocket/handler.rs`

**添加验证逻辑**:

```rust
fn handle_trade_message(
    &self,
    coin: &str,
    trade: &WsTradeData,
    instrument: &InstrumentAny,
) -> Result<TradeTick, String> {
    // Check if this is an outcome market
    if let Some(def) = self.get_instrument_def(coin) {
        if def.market_type == HyperliquidMarketType::Outcome {
            // Validate price is within expected range
            let price = Decimal::from_str(&trade.px)
                .map_err(|e| format!("Invalid price: {e}"))?;
            
            if price < dec!(0.001) || price > dec!(0.999) {
                log::warn!(
                    "Outcome market price {} outside normal range [0.001, 0.999] for {}",
                    price, coin
                );
            }
        }
    }
    
    // Continue with standard parsing
    parse_ws_trade_tick(trade, instrument, ts_init)
}
```

---

## 四、测试计划

### 4.1 单元测试

每个新增模块都需要对应的单元测试：

| 模块 | 测试文件 | 测试覆盖率目标 |
|------|----------|----------------|
| `validation.rs` | `tests/validation_tests.rs` | 90% |
| `execution.rs` | `tests/execution_tests.rs` | 85% |
| `settlement.rs` | `tests/settlement_tests.rs` | 90% |
| `parse.rs` | `tests/outcome_parse_tests.rs` | 80% |

### 4.2 集成测试

```rust
#[tokio::test]
async fn test_paper_trading_full_flow() {
    // Setup
    let mut paper_exec = OutcomePaperExecution::new(dec!(10000));
    let instrument = create_test_outcome_instrument();
    
    // Update market data
    paper_exec.update_bbo(
        instrument.id(),
        Price::from("0.60"),  // bid
        Price::from("0.62"),  // ask
    );
    
    // Submit buy order
    let order = create_limit_order(
        instrument.id(),
        OrderSide::Buy,
        Price::from("0.62"),
        Quantity::from("1000"),
    );
    
    let result = paper_exec.submit_order(&order, &instrument).unwrap();
    assert_eq!(result.status, OrderStatus::Filled);
    
    // Verify position
    let position = paper_exec.position(instrument.id()).unwrap();
    assert_eq!(position.entry_price, Price::from("0.62"));
    assert_eq!(position.size, Quantity::from("1000"));
    
    // Settle position
    let settlement = SettlementEngine::settle_position(position, true);
    assert_eq!(settlement.pnl, dec!(380));  // (1.0 - 0.62) * 1000
}
```

---

## 五、文档要求

### 5.1 代码文档

- 所有公共 API 必须包含完整的 doc comments
- 复杂逻辑需要 inline comments
- 示例代码展示典型使用场景

### 5.2 用户文档

- 更新 README.md 添加预测市场支持说明
- 创建示例脚本展示纸面交易用法
- 添加 troubleshooting 指南

---

## 六、验收清单

### 6.1 功能验收

- [ ] 可以解析预测市场 instrument 定义
- [ ] 可以提交纸面交易订单
- [ ] 订单价格范围验证正确（0.001-0.999）
- [ ] 保证金计算正确（全额抵押）
- [ ] 结算计算正确（二元结果）
- [ ] WebSocket 数据可以正确更新市场状态

### 6.2 代码质量验收

- [ ] 所有代码遵循项目代码风格
- [ ] 单元测试覆盖率 >= 80%
- [ ] 所有测试通过
- [ ] 无 clippy warnings
- [ ] 文档完整

### 6.3 集成验收

- [ ] 与现有 Hyperliquid adapter 兼容
- [ ] 不破坏现有功能
- [ ] Python 绑定正常工作（如适用）

---

## 七、风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 预测市场 API 未文档化 | 高 | 密切监控测试网，准备适配调整 |
| 结算逻辑复杂 | 中 | 充分单元测试，边界条件覆盖 |
| 与现有代码冲突 | 低 | 保持向后兼容，渐进式集成 |

---

## 八、后续工作

### 8.1 Phase 5（可选）: 实盘支持

- 添加 EIP-712 签名
- 实现真实订单提交
- 处理链上确认

### 8.2 Phase 6（可选）: 高级功能

- 多市场组合策略
- 自动到期检测
- 预言机结果监听

---

*文档结束*
