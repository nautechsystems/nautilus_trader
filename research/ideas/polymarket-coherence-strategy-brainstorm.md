# Polymarket 策略研究 brainstorm：概率相干性 / 结构套利

日期：2026-06-26  
状态：idea / brainstorm，非正式汇报正文

## 0. 一句话

Polymarket 可以从数学上看成一个带 bid/ask、费用、深度、结算语义和执行摩擦的有限事件概率系统。策略研究不一定从“预测单个市场方向”开始，也可以从“这些市场价格能不能同时来自同一个概率世界”开始。

核心问题：

```text
给定一组事件 A_1, ..., A_m 和它们的盘口价格，
是否存在一个概率测度 P，使得所有价格都能被 P 同时解释？
```

如果不能，可能出现：

1. 真套利；
2. 结构性 mispricing；
3. 语义 / 裁决 / 执行风险导致的假套利。

---

## 1. 数学核心：coherence / no-arbitrage

设最终世界状态为：

```text
ω ∈ Ω
```

每个 YES token 对应事件：

```text
A_j ⊂ Ω
```

payoff：

```text
1_Aj(ω) = 1 if ω ∈ A_j else 0
```

把所有 token 的 payoff 写成矩阵：

```text
M[ω, j] ∈ {0, 1}
```

若没有 bid/ask，只看价格向量 `p`，无套利要求：

```text
存在 q_ω >= 0, sum q_ω = 1
使得 p = M^T q
```

也就是说：

```text
价格向量 p 必须落在 payoff vectors 形成的 convex polytope 里。
```

若有 bid/ask，则要求：

```text
bid_j <= Σ_ω q_ω M[ω,j] <= ask_j
```

如果这个线性可行性问题无解，则报价不相干，存在 Dutch book / arbitrage certificate。

---

## 2. 对偶视角：套利组合

令：

```text
y_j >= 0  表示买入 token j
z_j >= 0  表示卖出 token j
```

组合 payoff：

```text
payoff(ω) = Σ_j M[ω,j] (y_j - z_j)
```

建仓成本：

```text
cost = Σ_j ask_j y_j - Σ_j bid_j z_j
```

套利条件：

```text
payoff(ω) >= 0  对所有 ω
cost < 0
```

解释：所有状态都不亏，但建仓时还能净收钱。

这和 `q` 的存在性是线性规划对偶关系，本质来自 Farkas lemma / separating hyperplane theorem。

---

## 3. 常见约束族

### 3.1 Complement：YES / NO

同一 binary market：

```text
YES + NO = 1
```

无套利区间：

```text
bid_yes + bid_no <= 1 <= ask_yes + ask_no
```

机会：

```text
ask_yes + ask_no < 1   买 YES + 买 NO
bid_yes + bid_no > 1   卖 YES + 卖 NO / split 后卖
```

这是最干净的协议层约束，适合用来验证盘口、费用、成交模型和回测 plumbing。

### 3.2 Partition / MECE

若事件：

```text
A_1, ..., A_n
```

互斥且穷尽：

```text
A_i ∩ A_j = ∅
A_1 ∪ ... ∪ A_n = Ω
```

则：

```text
Σ_i P(A_i) = 1
```

机会：

```text
Σ_i ask_i < 1   全买 YES
Σ_i bid_i > 1   全卖 YES
```

最大风险：穷尽性。若存在 hidden other / none / void / cancellation，则 `Σ p_i != 1` 未必是套利。

### 3.3 Implication

若：

```text
A ⊂ B
```

则：

```text
P(A) <= P(B)
```

机会：

```text
bid_A > ask_B
```

组合：买 B，卖 A。因为：

```text
1_B - 1_A >= 0
```

这类比 YES/NO 更有 alpha，因为非显然逻辑关系不一定被简单扫描器覆盖。

### 3.4 Threshold / CDF

若随机变量 X 有阈值市场：

```text
A_k = {X >= k}
```

则：

```text
k1 < k2 => P(X >= k1) >= P(X >= k2)
```

可以研究 survival curve / CDF：

```text
P(k1 <= X < k2) = P(X >= k1) - P(X >= k2)
```

负差分就是不相干。

适用：天气温度、crypto price、rate cuts、sports score、seat count 等。

### 3.5 Intersection / Union / Fréchet bounds

对任意 A, B：

```text
P(A ∩ B) <= P(A)
P(A ∩ B) <= P(B)
P(A ∪ B) >= P(A)
P(A ∪ B) >= P(B)
```

Fréchet bounds：

```text
max(0, P(A)+P(B)-1) <= P(A∩B) <= min(P(A), P(B))
max(P(A), P(B)) <= P(A∪B) <= min(1, P(A)+P(B))
```

若市场上存在 A、B、A and B、A or B，可以扫这些边界。

---

## 4. 研究价值最大的不是显然套利

显然的 YES/NO parity 或简单 overround/underround，别人也会扫，机会很快被吃掉。

更可能有价值的方向：

1. 非显然 implication；
2. threshold / bucket 曲线不光滑；
3. bucket 与 cumulative market 不一致；
4. neg-risk / multi-outcome group 的细节；
5. resolution rule 等价但市场没有意识到；
6. 语义复杂导致简单 bot 不敢吃的机会。

真正 edge 来自：

```text
更准的市场关系图
更准的 resolution rule parser
更好的费用 / 深度 / 成交模型
更快区分真机会和假机会
```

---

## 5. 建议落地路线

不要一开始直接做 trading bot。先做：

```text
market ontology + constraint scanner + historical replay evaluation
```

### 5.1 市场关系图

每个 market / token 标注：

```text
event_id
condition_id
token_id
question
resolution_rule
category
relation_type
group_id
semantic_confidence
```

关系类型：

```text
COMPLEMENT
PARTITION
IMPLICATION
THRESHOLD
BUCKET
UNION
INTERSECTION
NEG_RISK_GROUP
```

### 5.2 constraint scanner 输出

```text
constraint_type
market_group
markets
raw_edge
fee_adjusted_edge
depth_adjusted_edge
duration_seconds
max_size
fill_probability
semantic_confidence
resolution_risk
```

### 5.3 回测层使用当前 repo

用当前 L2 replay / strategy harness 做：

1. violation 出现时盘口是否真的可成交；
2. 机会持续多久；
3. edge 被费用和 spread 吃掉多少；
4. maker/taker 执行敏感性；
5. partial fill / queue ahead 假设下是否仍成立。

---

## 6. 优先级

P0：YES/NO parity scanner  
用途：验证数据、盘口、费用、tick、深度、fill model。

P1：同 event 多 outcome / neg-risk scanner  
用途：最接近真实结构套利。

P2：threshold / bucket curve scanner  
用途：适合天气、crypto、finance、sports 的因子研究。

P3：implication graph scanner  
用途：alpha 潜力大，但语义工程重。

P4：通用 LP solver  
用途：在关系图稳定后，作为 no-arbitrage certificate / Dutch book finder。

---

## 7. 风险和边界

数学上成立不等于实盘成立。必须显式建模：

```text
bid/ask spread
depth
fees
maker vs taker
latency
partial fill
queue priority
funding / capital lockup
settlement / redeem time
UMA / resolution risk
hidden other / void / cancellation
```

尤其是跨市场关系，最大的坑不是数学，而是 resolution rule。

---

## 8. 暂定结论

这条线值得做，但策略 edge 不会来自“大家都知道 YES+NO=1”。

更有价值的是：

```text
把 Polymarket 看成概率约束系统，
通过市场关系图找价格不相干，
再用 L2 replay 检验是否可成交。
```

这和当前回测 infra 的方向匹配：先把盘口、费用、成交模型、数据语义打牢，再往上叠 constraint scanner / basket strategy。
