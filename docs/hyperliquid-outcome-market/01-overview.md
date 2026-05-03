# Hyperliquid 预测市场（HIP-4 / Outcome Trading）集成分析

## 文档信息

- **创建日期**: 2026-05-03
- **分支**: feature/hyperliquid-outcome-market-analysis
- **状态**: 分析阶段
- **目标**: 评估在 Nautilus Trader 中集成 Hyperliquid 预测市场的可行性和实施方案

---

## 一、Hyperliquid 预测市场概述

### 1.1 什么是 HIP-4 / Outcome Trading

Hyperliquid 的 **HIP-4（Hyperliquid Improvement Proposal 4）** 于 2026 年 5 月正式上线主网，引入了原生的**二元预测市场合约**（Binary Prediction Market Contracts），直接与 Polymarket 和 Kalshi 竞争。

### 1.2 核心特性

| 特性 | 说明 |
|------|------|
| **价格范围** | 0.001 - 0.999（代表概率） |
| **结算货币** | USDH（Hyperliquid 原生稳定币） |
| **结算方式** | 二元结算（0 或 1 取决于预言机结果） |
| **杠杆** | **无杠杆、无强平、无追加保证金** |
| **费用** | 开仓零费用，仅平仓/结算时收费 |
| **订单簿** | **单一共享 CLOB**（价格即概率） |
| **保证金** | 统一交叉保证金（与现货、永续合约共享） |

### 1.3 与 Polymarket 的关键区别

| 特性 | Hyperliquid HIP-4 | Polymarket CTF |
|------|-------------------|----------------|
| 订单簿 | 单一共享（价格=概率） | 分开的 Yes/No 订单簿 |
| 持仓表示 | 直接 USDH 计价 | ERC-1155 SHARE 代币 |
| 买入 Yes | 以价格 P 买入，结算时价值 0 或 1 | 购买 SHARE 代币 |
| 买入 No | 以价格 (1-P) 卖出/做空 | 购买 No SHARE 代币 |
| **代币机制** | **无代币，纯现金结算** | 需要 redeem/split/merge |
| **结算** | 直接 USDH 增减 | 代币价值归 0 或 1 |

### 1.4 市场示例

```
市场: "BTC above $78,213 on May 3 at 11:30 AM?"
当前价格: 0.62 (62% Yes 概率)

订单簿:
  卖单 (Ask)              买单 (Bid)
  0.65 @ 1000             0.61 @ 1500
  0.63 @ 500              0.60 @ 3000
  0.62 @ 2000 (中间价)     0.58 @ 1000

盈亏计算:
- 用户以 0.62 买入 1000 Yes:
  * 如果到期 Yes: 盈利 (1.0 - 0.62) × 1000 = +380 USDH
  * 如果到期 No: 亏损 (0.0 - 0.62) × 1000 = -620 USDH
```

---

## 二、USDH 与资金流程

### 2.1 USDH 是什么

从 Hyperliquid API 数据发现：

```json
{
  "name": "USDH",
  "szDecimals": 2,
  "weiDecimals": 8,
  "index": 360,
  "tokenId": "0x54e00a5988577cb0b0c9ab0cb6ef7f4b",
  "isCanonical": false,          // 非官方标准代币
  "evmContract": {
    "address": "0x111111a1a0667d36bd57c0a9f569b98057111111"
  },
  "fullName": "USDH",
  "deployerTradingFeeShare": "0.0"
}
```

**关键发现**：
- USDH 是 Hyperliquid 生态内的代币，但不是官方发行的稳定币
- `isCanonical: false` 表示这是社区/第三方部署的代币
- 还有一个类似的 `USDHL` (Hyper USD)，也是非官方的

### 2.2 如何获得 USDH

**方式 1：现货交易兑换**
- 交易对：`HYPE/USDH` (pair index 232)
- 在 Hyperliquid 现货市场上用 HYPE 代币兑换 USDH

**方式 2：从外部桥接**
如果 USDH 是 Arbitrum 或其他链上的 ERC-20 代币，可以通过跨链桥接入。

### 2.3 充值流程

```
1. 访问 app.hyperliquid.xyz
2. 连接钱包（MetaMask、Rabby 等）
3. 选择 "Deposit"（入金）
4. 选择网络并发送资金
5. 资金到账后开始交易
```

**支持的网络**：
- Arbitrum（主要，推荐）
- Ethereum
- Optimism
- Base
- Solana
- Bitcoin
- Monad
- Plasma

**主要充值资产**：
- USDC（推荐主要抵押品）
- ETH（用于 Gas）
- BTC、SOL 等

### 2.4 费用结构

| 操作 | 费用 |
|------|------|
| 开仓 | **零费用** |
| 平仓 | 收取（具体费率） |
| 平台内转移 | 免费 |
| 提现到外部 | **$1 固定费用** |

**注意**：$1 提现费是指从 Hyperliquid 平台提取到**外部网络/地址**（如 Arbitrum 钱包、其他交易所），不是平台内部的市场间转移。

---

## 三、KYC 与地区限制

### 3.1 KYC 要求

**一般交易：不需要 KYC**

> "KYC is not required to trade on Hyperliquid."

**需要 KYC 的活动**：
- HYPE 质押
- 某些特定活动

**KYC 流程**：通过第三方提供商进行

### 3.2 地区限制

**明确禁止**：
- **美国用户**（US persons）被明确限制使用 Hyperliquid

**执行措施**：
- IP 封锁（IP blocking）
- 其他技术措施

**预测市场特殊限制**：

预测市场（二元期权）在以下地区通常受到严格监管或禁止：

| 地区 | 状态 |
|------|------|
| 美国 | 🚫 禁止 |
| 加拿大 | ⚠️ 受限（部分省份） |
| 英国 | ⚠️ 受限（FCA 监管） |
| 欧盟 | ⚠️ 受限（各国法规不同） |
| 中国 | 🚫 禁止 |
| 新加坡 | ⚠️ 受限（MAS 监管） |
| 澳大利亚 | ⚠️ 受限（ASIC 监管） |

**用户责任**：用户有责任遵守当地法律法规。

---

## 四、技术架构分析

### 4.1 API 基础设施

HIP-4 预测市场使用**与现有 Hyperliquid 市场相同的 API 基础设施**：

| 端点 | 用途 |
|------|------|
| Info endpoint | 查询市场数据、订单簿状态、持仓信息 |
| Exchange endpoint | 下单/撤单、管理持仓 |
| WebSocket | 实时订单簿更新和成交数据 |
| gRPC streaming | 低延迟数据流（用于自动化策略） |

### 4.2 新增 API 元素

- `outcomeMeta` 响应字段（目前仅测试网）
- 包含 outcome 逻辑的 Asset ID 编码

### 4.3 WebSocket 限制

| 限制 | 最大值 | 范围 |
|------|--------|------|
| 订阅数 | 1,000 | 每 IP 地址 |
| 连接数 | 100 | 每 IP 地址 |
| 消息数 | 2,000/分钟 | 每连接 |

**关键限制**：不支持批量订阅，每个交易对需要单独的订阅消息。

### 4.4 当前 Nautilus Hyperliquid 适配器状态

**已支持的市场类型**：

1. **永续合约 (Perp)**
   - 符号格式：`{BASE}-USD-PERP`（如 `BTC-USD-PERP`）
   - 原生符号：基础货币（如 `"BTC"`）
   - 资产索引：从 0 开始

2. **现货 (Spot)**
   - 符号格式：`{BASE}-{QUOTE}-SPOT`（如 `HYPE-USDC-SPOT`）
   - 原生符号：`@{pair_index}`（如 `"@107"`）或 `PURR/USDC`
   - 资产索引：`10000 + index`

**核心数据结构**：

```rust
// HyperliquidMarketType 枚举（当前）
pub enum HyperliquidMarketType {
    Perp,
    Spot,
}

// HyperliquidInstrumentDef
pub struct HyperliquidInstrumentDef {
    pub symbol: Ustr,           // 如 "BTC-USD-PERP"
    pub raw_symbol: Ustr,       // WebSocket 使用的原生符号
    pub base: Ustr,
    pub quote: Ustr,
    pub market_type: HyperliquidMarketType,
    pub asset_index: u32,
    pub price_decimals: u32,
    pub size_decimals: u32,
    pub tick_size: Decimal,
    pub lot_size: Decimal,
    pub max_leverage: Option<u32>,
    pub only_isolated: bool,
    pub is_hip3: bool,
    pub active: bool,
    pub raw_data: String,
}
```

---

## 五、关键发现与注意事项

### 5.1 关于 Yes/No 共享订单簿

**结论**：是的，单一共享订单簿

Hyperliquid 预测市场采用**单一中央限价订单簿（CLOB）** 设计：

- 没有独立的 "Yes Token" 或 "No Token"
- 用户的持仓直接以 USDH 计价
- 盈亏在结算时计算
- 买入 Yes = 以价格 P 买入，到期价值 0 或 1
- 买入 No = 以价格 (1-P) 卖出/做空

### 5.2 关于 tick_size_change 事件

**结论**：未发现此事件，需要进一步验证

现有代码中没有发现专门的价格精度变更事件。如果预测市场支持运行时调整 tick_size，可能通过以下方式：
- 重新订阅 instrument meta
- WebSocket 推送通知（待确认）

### 5.3 关于 Redeem / Split / Merge

**结论**：Hyperliquid 完全没有这些机制

这是 **Polymarket CTF** 特有的概念，Hyperliquid 采用完全不同的设计：

| 机制 | Hyperliquid | Polymarket |
|------|-------------|------------|
| Split | ❌ 不存在 | ✅ 存在 |
| Merge | ❌ 不存在 | ✅ 存在 |
| Redeem | ❌ 不存在 | ✅ 存在 |
| 持仓表示 | USDH 现金 | ERC-1155 代币 |

### 5.4 预测市场特殊之处

1. **价格范围**：必须验证在 0.001-0.999 之间
2. **无杠杆**：`max_leverage: Some(1)`
3. **全额抵押**：保证金 = 数量 × 价格
4. **到期结算**：二元结果（0 或 1）
5. **USDH 结算**：不是 USDC

---

## 六、参考资料

### 官方资源
- **主网应用**: https://app.hyperliquid.xyz
- **测试网**: https://app.hyperliquid-testnet.xyz
- **官方文档**: https://hyperliquid.gitbook.io/hyperliquid-docs
- **社区工具**: https://hip4.io

### 相关提交
- `13733fb9`: feat(hyperliquid): add allMids CustomData support
- `b8a74dea`: feat(deribit): add DVOL volatility index CustomData support

### 外部参考
- [HIP-4: Prediction Markets](https://www.dwellir.com/blog/what-is-hyperliquid-hip-4)
- [Hyperliquid Prediction Markets Live](https://coingape.com/hyperliquids-prediction-markets-upgrade-goes-live-on-mainnet)

---

*文档结束*
