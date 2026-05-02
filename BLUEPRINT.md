# KuaaMU Quant Engine — AI-Native 改造蓝图

**基座**: NautilusTrader (Rust Core + Python API)
**目标**: 为 Agent 量化交易构建高性能、可自主进化、链上可验证的执行基础设施
**交易所**: OKX (Demo Trading → Live)

---

## 设计理念

### 核心范式转移

| 传统量化系统 | AI-Native 量化引擎 |
|-------------|-------------------|
| 人类写策略 → 编译 → 运行 | Agent 读状态 → 输出意图 → 引擎编译执行 |
| REST/gRPC 调用（序列化损耗） | 共享内存 State-as-Context（零拷贝） |
| 硬编码风控规则（布尔阈值） | 可微分风险势能场（梯度感知） |
| 回测与实盘两套代码 | 同一状态空间，回测=冻结上下文，实盘=滚动上下文 |
| 固定策略参数 | Autoresearch 持续进化（Karpathy Ratchet） |
| 中心化信任（交易所/托管） | 链上声誉可验证（ERC-8004 + Staked Reputation） |

### 五大设计原则

- **P1 — 状态即上下文 (State-as-Context)**: 引擎状态以结构化 token 流暴露于共享内存，Agent 通过 mmap 零拷贝读取
- **P2 — 意图驱动执行 (Intent-Driven Execution)**: Agent 输出结构化意图，引擎编译为最优执行计划
- **P3 — 可微分风控 (Differentiable Risk)**: 风险是势能场而非硬墙，Agent 感知梯度自主规避
- **P4 — 自主进化 (Autoresearch Ratchet)**: 内置微回测循环，Agent 提出假设 → 验证 → 保留改进
- **P5 — 可验证信任 (Verifiable Trust)**: ERC-8004 Attestation 上链，声誉决定自主级别

---

## 系统架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                        AGENT SWARM (智能体集群)                      │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                │
│  │ 感知 Agent    │ │ 策略 Agent    │ │ 风控 Agent    │                │
│  │ (Perception) │ │ (Strategy)   │ │ (Risk)       │                │
│  │ 本地 LLM     │ │ Autoresearch │ │ 势能场监控    │                │
│  └──────┬───────┘ └──────┬───────┘ └──────┬───────┘                │
│         └─────────────────┴─────────────────┘                       │
│                           ↓                                         │
│                    ┌──────────────┐                                 │
│                    │ 执行 Agent    │                                 │
│                    │ (Execution)  │                                 │
│                    └──────┬───────┘                                 │
└───────────────────────────┼─────────────────────────────────────────┘
                            ↓ Intent (AgentIntent)
┌─────────────────────────────────────────────────────────────────────┐
│                    INTENT COMPILER (意图编译器)                      │
│  Stage 1: Template Matching (<1μs)                                  │
│  Stage 2: Almgren-Chriss Parameter Optimization (~1ms)              │
└───────────────────────────┬─────────────────────────────────────────┘
                            ↓ ExecutionPlan
┌─────────────────────────────────────────────────────────────────────┐
│                    NAUTILUS CORE (保留层，黑盒复用)                  │
│  MessageBus | ExecutionEngine | RiskEngine (硬底线)                 │
│  → OKX Adapter (Demo Trading / Live)                                │
└─────────────────────────────────────────────────────────────────────┘
                            ↓ 旁路输出
┌─────────────────────────────────────────────────────────────────────┐
│                    STATE ENCODER (状态编码器)                        │
│  OrderBook / Portfolio / Greeks → ContextWindow → /dev/shm mmap    │
└─────────────────────────────────────────────────────────────────────┘
                            ↓ 共享内存
┌─────────────────────────────────────────────────────────────────────┐
│                    REPUTATION LAYER (声誉层)                         │
│  ERC-8004 Attestation → 链上声誉 Registry → 自主级别 Slider        │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 模块改造清单

### 保留层 (零改动，直接复用)

| 模块 | 路径 | 复用理由 |
|------|------|---------|
| `nautilus_core` | `crates/core/` | UUID、时间戳、标识符、货币 |
| `nautilus_model` | `crates/model/` | 事件模型、订单类型、账户状态 |
| `nautilus_common` | `crates/common/` | MessageBus、缓存、时钟、日志、Actor |
| `nautilus_execution` | `crates/execution/` | 订单生命周期、适配器路由、OMS |
| `nautilus_network` | `crates/network/` | WebSocket/TCP、心跳、重连 |
| `nautilus_persistence` | `crates/persistence/` | Parquet 序列化、数据库存储 |

### 新增模块

| 模块 | 路径 | 职责 |
|------|------|------|
| `agent_swarm` | `crates/agent_swarm/` | Agent trait、SwarmCoordinator、IntentCompiler |
| `state_encoder` | `crates/state_encoder/` | 共享内存 ContextWindow 编码 |
| `risk_potential` | `crates/risk_potential/` | 可微分风险势能场 |
| `autoresearch` | `crates/autoresearch/` | 微回测 + Ratchet 进化 |
| `reputation` | `crates/reputation/` | ERC-8004 Attestation (P4) |

---

## 开发阶段

### P0: 基座熟悉 (现在-6月)
- [x] Fork NautilusTrader + 创建开发分支
- [x] 安装 Rust toolchain (rustc 1.95.0)
- [x] 安装 uv (Python 包管理)
- [ ] OKX Demo Trading API 接入验证
- [ ] 跑通 Nautilus 原生策略 + OKX 模拟盘
- [ ] 理解 MessageBus publish/subscribe 流程
- [ ] 理解 ExecutionEngine 订单生命周期

### P1: StateEncoder (7-8月)
- [ ] `state_encoder/` 模块实现
- [ ] `memmap2` + `rkyv` 双缓冲共享内存
- [ ] OKX WebSocket → ContextWindow 编码
- [ ] 本地 Llama/Qwen 读取共享内存并输出决策
- [ ] 验证 State-as-Context 可行性

### P2: AgentSwarm (9-10月)
- [ ] `agent_swarm/` 替换 Strategy 层
- [ ] 感知/策略/风控/执行四 Agent 协作
- [ ] IntentCompiler (两阶段: Template + Optimizer)
- [ ] SwarmCoordinator 多 Agent 调度
- [ ] 三层决策路由 (Rule / Fast LLM / Deep LLM)

### P3: 冻结 (11-12月)
- 停止开发，只维护；考研冲刺

### P4: Autoresearch + Reputation (明年寒假)
- [ ] `autoresearch/` 微回测 + Ratchet
- [ ] `reputation/` ERC-8004 Attestation
- [ ] 完整闭环

---

## OKX 适配

| 维度 | 说明 |
|------|------|
| 模拟盘 | Demo Trading API，`x-simulated-trading: 1` header 切换 |
| WebSocket | `/ws/v5/public` (市场数据) + `/ws/v5/private` (账户) |
| Algo 订单 | 原生支持 TWAP/Iceberg/Trigger，IntentCompiler 可直接映射 |
| 优势 | 同一套代码加 header 即可模拟盘/实盘切换 |

---

## 优化方案

### 1. IntentCompiler 两阶段编译
- Stage 1: Template Matching (确定性, <1μs)
- Stage 2: Almgren-Chriss 参数优化 (~1ms)

### 2. ContextWindow 双缓冲 + SeqLock
- 两块 ContextWindow 交替写入
- Agent 侧无锁读取，延迟 <50ns

### 3. Agent LLM 三层决策
- Layer 1: 规则引擎 (<1μs) — 95% 决策
- Layer 2: 小模型 Qwen3-8B (~50ms) — 4% 决策
- Layer 3: 大模型 Qwen3-27B (~500ms) — 1% 决策

### 4. RiskPotentialField 解析解
- `#[inline(always)]` + 值类型，零堆分配
- 对数壁垒函数，梯度可解析求导

### 5. Autoresearch 增量回放
- 只回放候选策略与基线的行为差异
- 10% 差异 → 10x 速度提升

---

## 技术栈

| 层级 | 技术选型 |
|------|---------|
| 核心引擎 | Rust (NautilusTrader) |
| 共享内存 | `memmap2` + `rkyv` |
| 并发 | `crossbeam-channel` + `parking_lot` |
| Agent LLM | `llama.cpp` (Qwen3.5-27B / Llama 3.3-70B) |
| 序列化 | `rkyv` (零拷贝) + `serde_json` (调试) |
| 链上交互 | `ethers-rs` (EVM) / `solana-client` |
| Python | uv 管理，Python 3.11+ |
| 交易所 | OKX API v5 |
| 监控 | `tracing` + `prometheus` |
| 回测数据 | Parquet (Nautilus 原生) |

---

## 评估体系

| 维度 | 指标 | 目标 |
|------|------|------|
| Re-Discovery | 复现经典策略的风险调整 IR | ≥ 0.8 |
| New Discovery | 超越基线的 IR 提升 | ≥ 5% |
| 跨 Regime 稳健性 | IR 变异系数 | < 0.3 |
| 执行保真度 | 回测 vs 纸上交易盈亏相关性 | > 0.85 |
| 状态一致性 | 共享内存读取延迟 | < 1μs |
| 风险可控性 | 硬底线触发次数 (100 次决策) | 0 |

---

## 止损条件

P1 结束时 (8月底)，若本地 LLM 决策质量无法超越简单均线策略：
- 暂停 Agent 层开发
- 保留 StateEncoder 作为独立项目（高性能数据基础设施）
- 考研后再继续
