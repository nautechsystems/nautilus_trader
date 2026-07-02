# Polymarket 支持范围与回测示例解读

本文回答三个问题：

1. `README.md` 里 Polymarket 标成 `stable`，到底稳定支持了什么。
2. Polymarket 的实时行情订阅，和 `PolymarketDataLoader` 把历史 trades 解析成 Nautilus `TradeTick`，是不是同一套/互相配合。
3. `examples/backtest/polymarket_simple_quoter.py` 这个完整 backtest 示例实际在做什么。

定位说明：这是按本次问题落在仓库根目录的分析备忘；长期维护时，`docs/integrations/polymarket.md` 仍应视为 canonical integration guide。若两者冲突，应先更新/核对 canonical guide，再同步本备忘。

## 结论摘要

`stable` 不是“覆盖 Polymarket 全部产品能力”，也不是“历史盘口、复杂订单、链上赎回都封装好了”。在本仓库语境里，`stable` 的定义是：该 integration 的功能集和 API 已经稳定，并经过开发者和用户合理程度测试，但仍可能有 bug（见 `README.md:122` 和 `README.md:133`）。

Polymarket integration 当前稳定覆盖的是 **Polymarket CLOB 上二元预测市场的基础交易生命周期**：

- 把 Polymarket outcome token 建模为 Nautilus `BinaryOption` instrument（`docs/integrations/polymarket.md:43-50`）。
- 加载/解析 instrument，创建 live data client 和 live execution client（`docs/integrations/polymarket.md:62-73`）。
- 订阅实时行情：quotes、trades、order book deltas；支持动态 subscribe/unsubscribe 和缺失 instrument 的 runtime auto-load（`docs/integrations/polymarket.md:606-648`）。
- 执行基础订单：`MARKET`、`LIMIT`，以及 `GTC`、`GTD`、`FOK`、`IOC`/Polymarket `FAK` 映射（`docs/integrations/polymarket.md:247-315`）。
- 支持 `post_only`，但只限 `GTC`/`GTD` limit orders（`docs/integrations/polymarket.md:291-296`）。
- 支持批量提交独立 limit orders，单次最多 15 笔；支持批量撤单（`docs/integrations/polymarket.md:326-332`）。
- 支持历史 trades 加载并解析成 Nautilus `TradeTick`，用于研究/回测（`docs/integrations/polymarket.md:1006-1024`、`docs/integrations/polymarket.md:1143-1183`）。

明确不属于完整支持范围的能力包括：stop / stop-limit / touched / trailing stop、reduce-only、订单修改、bracket/OCO/conditional linked orders、杠杆/保证金、链上 redemption/claim funds、CLOB price-history timeseries helper、历史 order-book snapshot helper，以及完整非 active order 历史（见 `docs/integrations/polymarket.md:247-332`、`docs/integrations/polymarket.md:533-550`、`docs/integrations/polymarket.md:1006-1024`）。

## 实时订阅和历史 TradeTick 是否“相互配合”

短答案：**它们在 Nautilus 数据模型层面兼容，但不是同一条数据管线；不会自动互相喂数据或做 live/backfill 拼接。**

### 共同点：最后都进入 Nautilus 的数据模型

实时和历史两条路径都会产生 Nautilus 可消费的数据对象，尤其是 `TradeTick`：

- 实时路径中，WebSocket 收到 Polymarket trade message 后，`PolymarketDataClient._handle_trade()` 调用 `ws_message.parse_to_trade_tick(...)`，再 `_handle_data(trade)` 推进 live data engine（`nautilus_trader/adapters/polymarket/data.py:785-792`）。具体 WebSocket trade 到 `TradeTick` 的字段映射在 `nautilus_trader/adapters/polymarket/schemas/book.py:269-302`。
- 历史路径中，`PolymarketDataLoader.load_trades()` 从 Data API 拉 raw trades、按 token/time 过滤、排序，然后调用 `parse_trades()`（`nautilus_trader/adapters/polymarket/loaders.py:469-528`）。`parse_trades()` 把 raw Data API trades 转成 `TradeTick`（`nautilus_trader/adapters/polymarket/loaders.py:818-889`）。

这意味着：策略/回测引擎看到的是 Nautilus 标准数据类型，尤其是可以基于 trade ticks 生成 tick bars 或触发策略逻辑。

### 不同点：数据来源和入口完全不同

实时订阅管线：

```text
Strategy subscribe_* command
  -> PolymarketDataClient._subscribe_quote_ticks/_subscribe_trade_ticks/_subscribe_order_book_deltas
  -> WebSocket subscribe(token_id)
  -> WebSocket messages
  -> QuoteTick / TradeTick / OrderBookDeltas
  -> live data engine
```

证据：

- `_subscribe_order_book_deltas()`、`_subscribe_quote_ticks()`、`_subscribe_trade_ticks()` 都先 `_ensure_instrument_loaded()`，再按 `token_id` 订阅 WebSocket（`nautilus_trader/adapters/polymarket/data.py:420-477`）。
- guide 也说明 data adapter 会动态订阅 instrument，并在 instrument 不在 cache 时通过 Gamma auto-load（`docs/integrations/polymarket.md:612-630`）。

历史 trades 管线：

```text
PolymarketDataLoader.from_market_slug(...)
  -> Gamma API / CLOB API 建 instrument + condition_id/token_id
  -> loader.load_trades()
  -> Data API /trades
  -> filter by token_id, client-side time filter, sort
  -> parse_trades()
  -> list[TradeTick]
  -> BacktestEngine.add_data(trades)
```

证据：

- `from_market_slug()` 先 fetch market slug，再 fetch CLOB market details，并选定 token（`nautilus_trader/adapters/polymarket/loaders.py:187-245`）。
- `fetch_trades()` 请求的是 `https://data-api.polymarket.com/trades`，分页取 raw trades（`nautilus_trader/adapters/polymarket/loaders.py:743-816`）。
- `load_trades()` 返回 “ready for backtesting” 的 `list[TradeTick]`（`nautilus_trader/adapters/polymarket/loaders.py:469-490`）。

### 关键边界：live data client 不负责历史 backfill

一个容易误会的地方是：live data client 有 `_request_trade_ticks()`，但它明确报错：`Cannot request historical trades: not published by Polymarket`（`nautilus_trader/adapters/polymarket/data.py:570-572`）。这说明在 live data client 的 request path 里，不能把 Polymarket 当成常规 venue 来请求历史 trade ticks。

历史 trades 的能力是由 `PolymarketDataLoader` 单独提供的，用于 research/backtesting，不是 live WebSocket subscription 的内置 backfill 机制。换句话说：

- 如果你在 live trading node 里 `subscribe_trade_ticks()`，拿到的是实时 WebSocket `last_trade_price` 事件。
- 如果你在 research/backtest 脚本里 `await loader.load_trades()`，拿到的是 Data API 历史 trades 转出来的 `TradeTick` 列表。
- 两者共享 instrument/id/price/qty/TradeTick 语义，但没有自动合流、自动补历史、自动续接 live 的逻辑。

如果需要“先历史回放，再接实时”，需要在应用层明确设计边界：先用 loader 拉历史 `TradeTick` 做 warmup/backtest，live node 再通过 data client 订阅实时；不要假设 adapter 会自动把 Data API 历史和 WebSocket 实时拼成一个连续 feed。

## `examples/backtest/polymarket_simple_quoter.py` 解读

这个示例不是 live trading 示例，而是一个最小化的 **历史成交 tick 回测** 示例。

### 1. 选一个 Polymarket market slug

脚本默认：

```python
MARKET_SLUG = "gta-vi-released-before-june-2026"
```

并提示可以用脚本查 active markets 或 BTC/ETH UpDown markets（`examples/backtest/polymarket_simple_quoter.py:48-53`）。

### 2. 用 `PolymarketDataLoader.from_market_slug()` 建 instrument

```python
loader = await PolymarketDataLoader.from_market_slug(market_slug)
instrument = loader.instrument
```

这一步会根据 slug 拉 market metadata 和 CLOB market details，选出一个 outcome token，并构造 Nautilus `BinaryOption` instrument（`examples/backtest/polymarket_simple_quoter.py:68-74`；loader 细节见 `nautilus_trader/adapters/polymarket/loaders.py:187-245`）。

回测已 resolved 的市场时要特别小心 look-ahead bias：resolved market payload 可能在 `instrument.info` 里包含答案相关字段。loader 支持 `sanitize_info=True`，用于在构造 instrument 前剥离这些 resolution-bearing fields（`nautilus_trader/adapters/polymarket/loaders.py:205-211`；文档说明见 `docs/integrations/polymarket.md:1096-1117`）。如果策略会读取 `cache.instrument(...).info`，应使用 `sanitize_info=True` 或明确保证策略不会读到答案字段。

### 3. 拉历史 trades 并解析成 `TradeTick`

```python
trades = await loader.load_trades()
```

脚本随后检查没有 trades 就直接报错（`examples/backtest/polymarket_simple_quoter.py:76-82`）。这一步使用的是 Polymarket Data API 的历史 trades，不是 WebSocket live subscription。

### 4. 创建普通 Nautilus `BacktestEngine`

```python
config = BacktestEngineConfig(trader_id=TraderId("BACKTESTER-001"))
engine = BacktestEngine(config=config)
```

然后添加 venue：

```python
engine.add_venue(
    venue=POLYMARKET_VENUE,
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,
    base_currency=USDC_POS,
    starting_balances=[Money(10_000, USDC_POS)],
)
```

见 `examples/backtest/polymarket_simple_quoter.py:84-95`。注意：脚本实际使用 `USDC_POS` 作为 base currency（`examples/backtest/polymarket_simple_quoter.py:40`、`examples/backtest/polymarket_simple_quoter.py:93-94`），而集成文档正文把 Polymarket collateral 描述为 pUSD（`docs/integrations/polymarket.md:43-50`、`docs/integrations/polymarket.md:97-107`）。因此不要从这个 backtest 示例反推 live Polymarket 账户/抵押币配置；live/canonical 语义仍以 pUSD 文档为准。这里的重点只是：本地 backtest engine 给 `POLYMARKET` venue 配了一个现金账户和初始余额。

### 5. 把 instrument 和历史 TradeTicks 放进回测引擎

```python
engine.add_instrument(instrument)
engine.add_data(trades)
```

这是 backtest 的核心：instrument 定义市场，`trades` 是历史成交事件流（`examples/backtest/polymarket_simple_quoter.py:97-99`）。

### 6. 用 trade ticks 聚合 100-tick bars，跑 EMA crossover 示例策略

```python
bar_type = BarType.from_str(f"{instrument.id}-100-TICK-LAST-INTERNAL")
strategy_config = EMACrossLongOnlyConfig(
    instrument_id=instrument.id,
    bar_type=bar_type,
    trade_size=Decimal(20),
    fast_ema_period=10,
    slow_ema_period=20,
)
```

含义：

- 输入数据是 `TradeTick`。
- Nautilus 内部按 `100-TICK-LAST-INTERNAL` 聚合 tick bars。
- 示例策略 `EMACrossLongOnly` 在这些 bars 上算 EMA crossover。

见 `examples/backtest/polymarket_simple_quoter.py:101-112`。

### 7. 运行回测并打印报告

脚本调用 `engine.run()`，然后打印 account、fills、positions 三类报告，最后 reset/dispose engine（`examples/backtest/polymarket_simple_quoter.py:114-136`）。

### 这个示例能说明什么

它说明 Polymarket adapter 已经把以下链路打通：

```text
market slug
  -> Polymarket instrument
  -> historical Data API trades
  -> Nautilus TradeTick
  -> BacktestEngine.add_data
  -> tick-bar strategy
  -> account/fill/position reports
```

它不说明：

- 可以从 Polymarket CLOB 拉完整历史 order book。
- 可以在 live client 里 request historical trades。
- 可以自动把历史 trades 和 WebSocket 实时 trades 拼接。
- 该策略适合真实交易；它只是示范数据接入和回测 plumbing。

## 实操建议

- 做 live：用 `PolymarketDataClient` / live node factory，订阅 `quote_ticks`、`trade_ticks` 或 `order_book_deltas`；需要提前加载或允许 runtime auto-load instrument。
- 做 research/backtest：用 `PolymarketDataLoader.from_market_slug()` 或 `from_event_slug()`，再 `load_trades()`，然后把 `TradeTick` 列表喂给 `BacktestEngine.add_data()`；若市场已 resolved 且策略可能读 `instrument.info`，使用 `sanitize_info=True` 或做等价防护，避免把结果答案泄露进回测。
- 做连续研究到实盘：可以复用相同 instrument id 和 strategy 逻辑，但要自己处理历史 warmup、实时订阅开始时间、重复 trade 去重和 gap 检测。
- 对高频/高活跃市场要谨慎：public Data API 的 offset pagination 有上限，loader 命中上限时只返回部分 trades 并发出 warning（`docs/integrations/polymarket.md:1178-1182`；实现见 `nautilus_trader/adapters/polymarket/loaders.py:788-800`）。
