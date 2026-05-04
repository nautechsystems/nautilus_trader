# Hyperliquid 预测市场集成分析

## 项目概述

本分支包含对 Hyperliquid 预测市场（HIP-4 / Outcome Trading）在 Nautilus Trader 中集成的完整技术分析。

## 文档列表

| 文档 | 内容 |
|------|------|
| [01-overview.md](./01-overview.md) | Hyperliquid 预测市场概述、USDH、KYC、地区限制 |
| [02-paper-trading-implementation.md](./02-paper-trading-implementation.md) | 纸面交易实现方案、代码改造清单 |
| [03-development-plan.md](./03-development-plan.md) | 详细开发计划、代码规范、任务分解 |

## 关键发现摘要

### 1. 预测市场特性

- **价格范围**: 0.001 - 0.999（概率）
- **结算货币**: USDH
- **订单簿**: 单一共享 CLOB（价格即概率）
- **杠杆**: 无（全额抵押）
- **结算**: 二元（0 或 1）
- **费用**: 开仓零费用

### 2. 与 Polymarket 的关键区别

| 特性 | Hyperliquid | Polymarket |
|------|-------------|------------|
| 订单簿 | 单一共享 | Yes/No 分离 |
| 代币机制 | ❌ 无（纯现金） | ✅ ERC-1155 SHARE |
| Redeem/Split/Merge | ❌ 不存在 | ✅ 必需 |
| 结算 | 直接 USDH | 代币赎回 |

### 3. 纸面交易最小改造

```
复用（无需改动）:
  - WebSocket 订阅方法
  - HTTP 客户端
  - 消息解析逻辑
  - Instrument 缓存

新增:
  - HyperliquidMarketType::Outcome
  - OutcomeMeta / OutcomeAsset 数据结构
  - OutcomePaperExecution 模拟执行器
  - 价格范围验证（0.001-0.999）
  - 二元结算逻辑
```

## 快速开始

### 查看分析文档

```bash
# 概述文档
cat docs/hyperliquid-outcome-market/01-overview.md

# 实现方案
cat docs/hyperliquid-outcome-market/02-paper-trading-implementation.md
```

### 估计工作量

- **纸面交易**: ~22 小时（3 天）
- **完整实盘支持**: +40 小时

## 参考资源

- [Hyperliquid 官方文档](https://hyperliquid.gitbook.io/hyperliquid-docs)
- [HIP-4 介绍](https://www.dwellir.com/blog/what-is-hyperliquid-hip-4)
- [预测市场上线公告](https://coingape.com/hyperliquids-prediction-markets-upgrade-goes-live-on-mainnet)

---

*创建日期: 2026-05-03*
