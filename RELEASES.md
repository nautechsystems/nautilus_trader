# NautilusTrader 1.226.0 Beta

Released on TBD (UTC).

### Enhancements
- Added `Portfolio::mark_values`, `equity`, and `missing_price_instruments` queries for Rust and Python
- Added `instrument_status` / `instrument_statuses` cache queries and auto-caching in the data engine (#3858)
- Added `environment` enum config for BitMEX, Deribit, dYdX, Hyperliquid, and OKX adapters
- Added `BybitEnvironment` to `BybitDataClientConfig` and `BybitExecClientConfig`
- Added missing config values to `LiveExecEngineConfig` (#3841), thanks @Javdu10
- Added `calculate_commission` to `ExecutionClient` for venue-specific reconciliation fills
- Added PyO3 bindings for `DataEngineConfig`, `ExecutionEngineConfig`, and `OrderEmulatorConfig` so they can be constructed from Python
- Added `cache`, `msgbus`, `data_engine`, `exec_engine`, and `portfolio` keyword arguments to `BacktestEngineConfig` Python constructor
- Added `MarginAccount.margin_for_currency` + `margin_init/maint_for_currency` helpers for cross-margin queries
- Added `MarginAccount.total_margin_init(currency)` / `total_margin_maint(currency)` summing both margin buckets
- Added `MarginAccount.account_margins`, `account_margins_init/maint`, and `clear_account_margin` accessors
- Added `transport-sockudo` feature with `TransportBackend` runtime selector for the WebSocket transport (Rust)
- Added `TransportBackend` PyO3 enum and `WebSocketConfig.backend` kwarg for backend selection from Python
- Added custom upgrade-header support on the sockudo backend so adapters carry the same `User-Agent` and per-venue headers on both backends (#3932), thanks @sunlei
- Added `WebSocketConfig.proxy_url` for HTTP `CONNECT` proxy tunneling with basic-auth support
- Added Betfair tiered tick scheme to `BettingInstrument` for ladder-snapped pricing
- Added Binance Futures `use_trade_lite` config to opt into low-latency `TRADE_LITE` fills (Rust, default `False`)
- Added Binance `proxy_url` plumbing for market and user-data WS streams (#3937), thanks for reporting @huangqingchi
- Added Bybit user-related endpoints (#3894), thanks @sunlei
- Added Bybit `BybitPositionIdx` enum and `bybit_resolve_position_idx` PyO3 helper
- Added Coinbase initial integration adapter (Rust)
- Added `DydxNetwork` re-export on the `nautilus_trader.adapters.dydx` package
- Added Hyperliquid historical funding rates via `fundingHistory` info endpoint
- Added Hyperliquid configurable MARKET slippage (`market_order_slippage_bps`) with per-order override
- Added Hyperliquid `OrderBookDepth10` subscription backed by the `l2Book` feed
- Added Hyperliquid `nSigFigs` / `mantissa` L2 precision controls via `subscribe_params`
- Added Interactive Brokers Rust adapter with PyO3 compatibility layer (#3864), thanks @faysou
- Added Kraken xStocks tokenized asset support for spot market data, order submission, and futures instruments
- Added OKX option greeks support for both Black-Scholes and price-adjusted conventions on every tick
- Added OKX `params["greeks_convention"]` (string or list) to narrow option greeks subscriptions
- Added OKX `transport_backend` config to switch websockets between `Tungstenite` and `Sockudo` backends
- Added Polymarket game_id and fee_schedule to instrument info (#3811), thanks @Javdu10
- Added Polymarket batch `SubmitOrderList` via `POST /orders` for limit-order batches (Rust)
- Added Polymarket WebSocket `idle_timeout_ms` for zombie detection (#3908), thanks for reporting @camarigor
- Added Polymarket WebSocket `proxy_url` plumbing
- Added Polymarket `pUSD` collateral currency (`Currency::pUSD()` in Rust, `pUSD` in Python) for the CLOB V2 cutover
- Added configurable `compression` for Tardis Machine replay, defaulting to `zstd`
- Added `ExecutionReport::OrderWithFills` and `send_order_with_fills` emitter for bundled status + fill reconciliation
- Added ADL / liquidation detection and logging across Binance, Bybit, OKX, BitMEX, Hyperliquid, Deribit, and dYdX
- Added Binance Futures COIN-M `delivery_autoclose-` prefix recognition for expiring contract auto-close events
- Added Bybit `adlRankIndicator` warning log when an open position is ranked 4 or higher (next to deleverage)
- Added Hyperliquid liquidation metadata logging on fills and `userEvents.liquidation` routing
- Added Hyperliquid `Auto-Deleveraging` fill detection with warn logging on HTTP and WebSocket paths

### Breaking Changes
- Added `Option<&AccountId>` to Rust `Portfolio::unrealized_pnls`, `realized_pnls`, `total_pnls`; pass `None` to keep prior behavior
- Added `backend: TransportBackend` to `WebSocketConfig`; struct-literal callers must add the field (Rust)
- Added `proxy_url: Option<String>` to `WebSocketConfig`; struct-literal callers must add the field (Rust)
- Migrated Polymarket adapter to CLOB V2: new EIP-712 domain version `2`, new exchange contract addresses, `timestamp`/`metadata`/`builder` order fields replace `taker`/`nonce`/`feeRateBps`, and pUSD replaces USDC.e as collateral; the Python adapter now uses `py-clob-client-v2`
- Consolidated adapter HTTP and WebSocket proxy plumbing onto a single `proxy_url` field, replacing the prior `http_proxy_url` / `ws_proxy_url` split across adapter Rust and Python configs
- Removed `DockerizedIBGatewayConfig::from_env_or_defaults` (Rust); use the bon builder or `Default::default`, which still falls back to `TWS_USERNAME`/`TWS_PASSWORD`
- Removed `OrderMatchingEngineConfig::new` and `with_price_protection_points` (Rust); use `OrderMatchingEngineConfig::builder()` instead
- Removed `BlockchainDataClientConfig::new`, `BlockchainExecutionClientConfig::new`, and `DexPoolFilters::new` (Rust); use the corresponding `::builder()` instead
- Removed `DeribitExecClientConfig::new` and `HyperliquidExecClientConfig::new` convenience constructors (Rust); use the `::builder()` instead
- Removed `DataEngineConfig::new` 12-arg positional constructor (Rust); use `DataEngineConfig::builder()` instead
- Removed synthetic `ACCOUNT-*` placeholders from margin adapters; `MarginBalance` emits with currency only
- Removed `nautilus_system::factories` module; import factory traits from `nautilus_common::factories` (Rust)
- Removed `indicators` from `nautilus-common` default features; enable with `features = ["indicators"]` (Rust)
- Renamed Python `DatabaseConfig.timeout` to `connection_timeout` and `response_timeout` to match the Redis/PyO3 wire schema
- Replaced `is_sandbox: bool` with `environment: AxEnvironment` on `AxDataClientConfig` and `AxExecClientConfig` (Rust and Python), aligning with the Binance/Bybit/Kraken adapter pattern. Default is `Sandbox`.
- Changed `BacktestEngine::add_venue` and `SimulatedExchange::new` (Rust) to take `SimulatedVenueConfig` (bon builder)
- Changed Interactive Brokers Rust configs to use bon builders: `InteractiveBrokersDataClientConfig`, `InteractiveBrokersExecClientConfig`, `InteractiveBrokersInstrumentProviderConfig`, and `DockerizedIBGatewayConfig`
- Changed `get_cached_bybit_http_client` signature: replaced `demo`/`testnet` bools with `environment: BybitEnvironment`
- Changed `UnsubscribeBookSnapshots` to require `interval_ms` for exact snapshot interval unsubscribe (Rust)
- Changed `OrderError::Invariant` variant to wrap `CorrectnessError` instead of `anyhow::Error` (Rust)
- Changed `HyperliquidEip712Signer::new()` to return `Result` and take `&EvmPrivateKey` (Rust)
- Changed `HyperliquidExchangeRequest::new/with_vault` to accept `HyperliquidSignature` directly (Rust)
- Changed Binance USD-M Futures WebSocket URLs from `/ws` to `/market/ws` and `/private/ws`
- Changed Cap'n Proto and SBE wire formats to preserve `Option` state (unstable, may change)
- Changed Python and Serde-backed Rust config decoding to reject unknown fields, so stale or misspelled keys now fail fast during config parsing
- Changed `MarginBalance.instrument_id` to optional; `None` marks account-wide (cross margin) entries keyed by currency
- Changed `MarginAccount.margins_init`/`margins_maint` to per-instrument only; use `account_margins_*` for cross margin
- Changed Binance Futures COIN-M to emit one `MarginBalance` per base coin (previously hardcoded USDT)
- Changed matching-engine `TradeId` format to `T-{hash}-{count}` from `{venue}-{raw_id}-{count}`; `ts_init`-keyed
- Changed `use_random_ids` to no longer govern `TradeId`; flag still affects `VenueOrderId` and `PositionId`
- Changed workspace `nautilus-live` to `default-features = false`; enable `features = ["node"]` for `LiveNode` (Rust)
- Changed adapter `LiveNode` examples to require `--features examples` to build (Rust)
- Changed `ParquetDataCatalog::to_object_path` and `to_object_path_parsed` to return `anyhow::Result` so cross-store URIs surface as errors instead of silently rewriting against the catalog bucket (Rust)
- Changed prefixed remote catalogs (`s3://bucket/base/path`, etc.) to read and write under their declared URI prefix; data previously written to the bucket root by the prior buggy behavior will not be discovered after upgrading and must be moved into the prefix (#3930)

### Security

### Fixes
- Fixed sockudo WebSocket backend dropping handshake leftover bytes when the server piggybacks the first frame on the 101 response (#3932), thanks @sunlei
- Fixed account state regeneration dropping account-wide margins on every fill across live and backtest paths
- Fixed `AccountState` to accept empty `balances` and `margins`
- Fixed `FillModel` determinism via `IndexMap` in `OrderMatchingEngine` (#3914), thanks for reporting @timkoopmans
- Fixed `mark_values`/`equity` keying by base currency when conversion is off; now keys by settlement currency
- Fixed `PortfolioAnalyzer` AttributeError on `MaxDrawdown`/`CAGR`/`CalmarRatio` (#3941), thanks for reporting @a1zb2yc3z
- Fixed `stop_timer` in `TimeBarAggregator` (#3822), thanks @faysou
- Fixed `RiskEngine` applying base `min_quantity`/`max_quantity` bounds to quote-denominated orders
- Fixed backtest `OrderMatchingEngine` treating `quote_quantity=True` orders as base quantity; the quote notional is now converted to a base quantity before fill simulation (#3873), thanks for reporting @fedoraiver
- Fixed `subscribe_option_chain` hanging on bootstrap in backtest (#3938), thanks for reporting @aaurix
- Fixed backtest option expiry fills missing from cache and fills report (#3939), thanks for reporting @hotelmike
- Fixed backtest physical option assignment closing the option leg at the opening premium (#3948), thanks for reporting @hotelmike
- Fixed `DataBackendSession` GIL deadlock when streaming custom data types (#3847), thanks for reporting @GianC0
- Fixed `BacktestNode` streaming with mixed built-in and custom data types (#3853), thanks for reporting @GianC0
- Fixed `DataBackendSession` chunked streaming memory leak causing RSS growth (#3889), thanks for reporting @GianC0
- Fixed book snapshot subscriptions to preserve exact `(instrument_id, interval_ms)` semantics for shared intervals and exact unsubscribe handling (Rust) (#3823), thanks for reporting @dwolfesberger
- Fixed WebSocket auth state during reconnection for Bybit, OKX, and Deribit (#3820), thanks for reporting @KaizynX
- Fixed WebSocket `idle_timeout_ms` reset on `Ping`/`Pong` keep-alive frames (#3907), thanks for reporting @camarigor
- Fixed `TradingNodeConfig.parse` dropping importable live client config `path` and `factory` fields during raw config decoding
- Fixed `OrderTriggered` ValueError on market-style stop orders (#3812), thanks for reporting @jindrichsirucek
- Fixed `consolidate_data_by_period` pairwise merging on fragment-per-flush catalogs (#3857), thanks for reporting @M-Advis
- Fixed `consolidate_data_by_period` destroying data on repeat runs and when straddling files spanned the consolidation window, mirrored in the Rust catalog backend (#3883), thanks @M-Advis
- Fixed `ParquetDataCatalog.get_intervals(identifier=None)` on per-identifier data (#3903), thanks for reporting @GianC0
- Fixed `ParquetDataCatalog.consolidate_data` raising `IndexError` when the start/end range did not overlap any files, and `consolidate_catalog_by_period` aborting the loop on the first unrecognized directory rather than skipping it (#3910), thanks for reporting @M-Advis
- Fixed remote catalog object paths under URI prefix so writes and reads under `s3://bucket/base/path` (and other remote schemes) no longer collapse to the bucket root (#3930), thanks @fedoraiver
- Fixed S3-backed custom data queries and remote Feather discovery (#3931), thanks for reporting @fedoraiver
- Fixed `FeatherWriter` writing 0-precision metadata on leading `CLEAR` delta (#3913), thanks for reporting @fedoraiver
- Fixed empty error log on `TradingNode` clean shutdown from `CancelledError` (#3862), thanks for reporting @jxstanford
- Fixed `Symbol` and `PositionId` deserialize of non-ASCII escaped strings (#3893), thanks for reporting @volemont
- Fixed execution engine ignoring user-supplied `position_id` from `submit_order` (Rust)
- Fixed `ExecutionEngine` leg-fill position events not publishing to subscribers (#3939)
- Fixed cache load not repairing OTO contingent child `position_id` after a partial fill-time crash (Rust)
- Fixed `TestDataGenerator.generate_trade_ticks` using random UUID4; now sequences deterministic `T-{idx}` IDs
- Fixed reconciliation IDs non-deterministic across restarts (#3878), thanks for reporting @peanut-copilot
- Fixed reconciliation synthetic `OrderStatusReport` now propagates fill price to `avg_px` for downstream inferred fills
- Fixed `reconcile_fill_report` dropping fills for unknown orders; now bootstraps external orders for venue closures
- Fixed PyO3 `InstrumentStatus` persistence and backtest streaming through `ParquetDataCatalog` (#3855)
- Fixed PyO3 `LiveNode` `request_bars()` historical callbacks dropped during startup warmup (#3825), thanks @BurnOutTrader
- Fixed PyO3 `DataActor` missing `on_historical_funding_rates` and `on_historical_data` forwarding `None`
- Fixed PyO3 crypto instrument `from_dict` for unregistered base/underlying codes (#3882), thanks for reporting @volemont
- Fixed PyO3 catalog `instruments()` failing on unregistered currencies (#3898), thanks for reporting @volemont
- Fixed PyO3 `from_dict` on non-ASCII strings via `ensure_ascii=False` in `json.dumps` (#3895), thanks @costajohnt
- Fixed Betfair event order: `Instrument` now emits before `InstrumentStatus`/`InstrumentClose` within each MCM
- Fixed Betfair scratched runners (`Removed`/`RemovedVacant`) emitting close only at market close; now fire immediately
- Fixed Betfair non-snapshot book deltas emitting inline; now tailed after trades/tickers to match Python semantics
- Fixed Betfair BSP deltas emitting before book deltas; now tailed after book deltas within each MCM
- Fixed Betfair order rejection reason dropping instruction-level `errorMessage` detail
- Fixed Betfair `query_order` to emit status reports via `customer_order_ref` and `bet_id` lookups (Rust)
- Fixed Binance user data stream not recovering after keepalive failure (#3861), thanks for reporting @KaizynX
- Fixed Binance Futures user data stream event loss during listen key rotation (#3861), thanks for reporting @KaizynX
- Fixed Binance Futures WebSocket trades by forcing `@aggTrade` (#3861), thanks for reporting @KaizynX
- Fixed Binance Futures exchange-generated fills losing real `trade_id` and `commission` by bundling status + fill
- Fixed Binance Ed25519 detector silently accepting base64 HMAC secrets as Ed25519 keys (Rust)
- Fixed Binance HTTP request Ed25519 signature URL-encoding in query strings (Rust)
- Fixed Binance Futures USD-M `cancel_all_orders` silently failing; routes through HTTP (WS API does not support it)
- Fixed Binance Futures `TRADE_LITE` user data events logging "Unknown event type" warnings on every fill
- Fixed Binance USD-M Futures WebSocket routing for `fstream-mm` and `fstream-auth` hosts
- Fixed BitMEX trade ID fallback using random UUID4 when `trdMatchID` missing; now hashed from trade fields
- Fixed Bybit demo mode websocket data URLs (#3742), thanks for reporting @jindrichsirucek
- Fixed Bybit position deserialization for closed positions (#3836), thanks for reporting @pusteckiy
- Fixed Bybit perpetual instrument status to emit `PreClose` when scheduled for delisting (#3829), thanks @dxwil
- Fixed Bybit `load_all_async` dropping `base_coin` filter for options (#3865), thanks for reporting @Baerenstein
- Fixed Bybit `InstrumentStatus` messages silently dropped instead of forwarded to the data engine
- Fixed Bybit and Deribit option chain example `subscribe_option_chain` call (#3887), thanks @sunlei
- Fixed Bybit margin missing for accounts with orders but no positions (#3725), thanks for reporting @marco-rigoni
- Fixed Bybit JSON pong websocket frames not being skipped before classification (#3936), thanks @sunlei
- Fixed Bybit hedge mode `positionIdx` rejection when `position_mode` set (#3944), thanks for reporting @pusteckiy
- Fixed Bybit execution client not applying configured leverage, position mode, or margin mode on connect (Rust)
- Fixed Databento CMBP1 and TCBBO trade IDs using random UUID4 instead of deterministic hash of trade fields
- Fixed Databento dropping `start_ns` after session start; now logs error (#3877), thanks for reporting @jxstanford
- Fixed Deribit mark/index price subscriptions silently dropping data in Python (#3821), thanks for reporting @linimin
- Fixed Deribit `StopMarket` `OrderRejected` on `market_price` price field (#3925), thanks for reporting @marco-rigoni
- Fixed dYdX `generate_order_status_report` fetching only the first order and missing later matches in the response
- Fixed dYdX orderbook snapshots missing `F_SNAPSHOT` flag on deltas; empty-book Clear now emits `F_SNAPSHOT | F_LAST`
- Fixed dYdX crossed-book resolution stripping `F_SNAPSHOT` from synthetic uncrossing deltas and the terminator
- Fixed dYdX trade-tick pagination dedup missing non-adjacent duplicates across page boundaries
- Fixed dYdX trade-tick pagination overshooting target `end` block from fixed block-time estimate
- Fixed dYdX crossed-book size arithmetic using `f64` subtraction; now uses `Decimal` at full precision
- Fixed dYdX position reports overriding venue `side` from `size` sign; venue side now preserved end-to-end
- Fixed dYdX `DydxAdapterConfig` defaulting to mainnet URLs regardless of `network`; added `for_network` helper
- Fixed Hyperliquid `LiveNode` bootstrap panic on HIP-3 instrument symbols containing `*`/`?` (e.g. `dex:STREAMABCD****-USD-PERP`) by substituting wildcard bytes with `x` in `InstrumentId.symbol` while preserving the venue-official name on `raw_symbol` (#3896), thanks for reporting @daiwanwei
- Fixed Hyperliquid bracket order submission grouping (#3810), thanks for reporting @jindrichsirucek
- Fixed Hyperliquid modify cancel-replace emitting stale `OrderCanceled` (#3827), thanks for reporting @P1YU5H-50N1
- Fixed Hyperliquid order status query for closed orders (#3879), thanks for reporting @pusteckiy
- Fixed Hyperliquid batch cancel silently dropping per-item errors (#3879), thanks for reporting @pusteckiy
- Fixed Hyperliquid Rust `query_order` handler to emit status reports (#3879), thanks for reporting @pusteckiy
- Fixed Hyperliquid `request_account_state` discarding parsed margins (#3725), thanks for reporting @marco-rigoni
- Fixed Hyperliquid `cancel_all_orders` dropping per-order rejection events on partial or transport failure
- Fixed Hyperliquid `request_trades` silently returning empty; now bails explicitly
- Fixed Hyperliquid `Auto-Deleveraging` fill direction deserialization (#3922), thanks for reporting @AlphaTraderK
- Fixed IB Gateway Docker image failing on ARM64 hosts (#3813), thanks for reporting @Baki-0501
- Fixed Interactive Brokers rejecting negative average fill price on combo/spread net-credit fills (#3884), thanks @faysou
- Fixed Interactive Brokers position reconciliation `TypeError` when `priceMagnifier` is `None` (#3885), thanks @davidsblom
- Fixed Kraken Futures limit order `OrderUpdated` panic from wire `stop_price: 0.0` treated as trigger price
- Fixed Kraken Futures fast-fill market orders resolving as rejected during order status reconciliation (#3870), thanks for reporting @Stamppot82
- Fixed Kraken Futures margin-account balance parse violating the `AccountBalance` invariant (`total == locked + free`) when Kraken's `af` field and the derived `amount - af` round independently at the currency precision
- Fixed Kraken Spot quote-quantity orders never reaching terminal state from base/quote size mismatch
- Fixed Kraken Spot ticker `QuoteTick.ts_event` using local init time instead of the exchange `timestamp` field (#3926), thanks @ptzafos
- Fixed Kraken trade dedup clearing the entire set at capacity instead of evicting the oldest entry
- Fixed Kraken Futures `AccountBalance` invariant panic on margin parse, thanks @Stamppot82
- Fixed Kraken Futures WebSocket re-authentication deadlock on reconnect (#3871), thanks for reporting @Stamppot82
- Fixed OKX option greeks not forwarded due to inaccessible Cython `cdef` subscription attribute
- Fixed OKX option greeks emitting `BlackScholes` convention regardless of subscribed greeks type
- Fixed OKX order identity registration race during concurrent order submission (Rust)
- Fixed OKX algo orders missing from order status reconciliation reports
- Fixed OKX spot margin position reconciliation preferring `CurrencyPair` with USDT/USDC/USD quote over alternatives
- Fixed OKX index-price subscription refcount leaking across reconnect and concurrent transitions
- Fixed OKX option summary subscription refcount not rolling back on subscribe failure
- Fixed OKX duplicate fills from empty `trade_id` using deterministic synthesized id instead of random UUID
- Fixed OKX panics on unmapped `OrderStatus` and empty `OptionType` values via `TryFrom` conversion (Rust)
- Fixed OKX `InstrumentStatus` messages logged as unhandled instead of forwarded to the data engine
- Fixed OKX `query_order` to emit status reports by merging regular and algo order lookups (Rust)
- Fixed Polymarket commission formula and fee source for fills (#3838), thanks for reporting @santivazq
- Fixed Polymarket reconciliation fills using incorrect commission (#3860), thanks for reporting @fedoraiver
- Fixed Polymarket instrument `min_quantity` denying market orders via limit-order shares rule (#3874), thanks for reporting @fedoraiver
- Fixed Polymarket `request_instrument(s)` dropping WS via stale `token_meta` (#3900), thanks for reporting @fedoraiver
- Fixed Polymarket `parse_to_quote_ticks` using changed level as top of book (#3905), thanks for reporting @camarigor
- Fixed Polymarket `parse_to_snapshot` missing `F_SNAPSHOT` flag on CLEAR and intermediate ADD deltas
- Fixed Polymarket `parse_to_deltas` flagging `F_LAST` on every delta instead of only the final one
- Fixed Polymarket `parse_to_trade_tick` using `uuid.uuid4()`, producing non-deterministic trade IDs
- Fixed Tardis replay handling of sparse `book_snapshot_*` levels (#3953), thanks for reporting @a1zb2yc3z
- Fixed Tardis trade ID fallback using random UUID4 when venue `id` missing/empty (CSV and WebSocket parsers)

### Internal Improvements
- Added `AccountBalance::from_total_and_locked` and `AccountBalance::from_total_and_free`, and migrated adapter balance parsing to preserve the `total == locked + free` invariant at currency precision (Rust)
- Added typed `CorrectnessError` enum to replace `anyhow::Error` in `correctness` helpers (Rust)
- Added `CorrectnessResultExt::expect_display` for display-formatted panics on typed correctness errors (Rust)
- Added deterministic simulation testing (DST) re-export module gated behind `simulation` feature (Rust)
- Added `wall_clock_now` seam in `nautilus-core` for virtual time under simulation (Rust)
- Added `biased` to `tokio::select!` blocks in network and live crates for deterministic poll order
- Added `nautilus_network::transport` module with `Message`/`TransportError`/`WsTransport` for future backend swap (Rust)
- Added neutral `Message`/`TransportError` re-exports on `nautilus_network` to ease future backend swaps (Rust)
- Added engine config methods on PyO3 `LiveNodeBuilder` (#3848), thanks @BurnOutTrader
- Added read-only `params()` accessor to `SubscribeCommand` and `TradingCommand` (#3846), thanks @faysou
- Added `ShutdownSystem` handling via `commands.system.shutdown` pub/sub topic, wired to kernel, backtest, and live (Rust)
- Added PyO3 `DataActor` parity with v1 for `publish_data`, `publish_signal`, `subscribe_signal`, `unsubscribe_signal`, `add_synthetic`, and `update_synthetic` (Rust)
- Added per-currency account-wide margin storage to `MarginAccount`, routing event margins by `instrument_id` presence
- Added Architect AX unit and integration tests for execution, request filters, and WebSocket parsers
- Added dYdX debug logging to `generate_order_status_report` showing filter scope and `page_full` on `None` results
- Added Polymarket `determine_trade_id` helper with FNV-1a (Rust) and blake2b (Python) deterministic hashing
- Added Hyperliquid criterion benchmarks for L1 signing path
- Added Hyperliquid integration tests for funding rates, trades, cancel-all, and `handle_l2_book` routing
- Added Hyperliquid `minTradeSpotNtlRejected` order status and `Unknown` liquidation method fallback
- Added Binance unit tests for spot/futures dispatch dedup, post-only rejection, and value conversions
- Added `derive_trade_id` FNV-1a helpers in BitMEX and Tardis common parse modules for deterministic fallback
- Added `derive_cmbp_trade_id` in Databento decode for schemas without a native trade ID
- Added property-based tests for Databento trade ID derivation (stability and 16-hex format)
- Added Rust/Python parity tests pinning matching-engine `TradeId` format across language bindings
- Added `node` feature to `nautilus-live` gating `builder`, `config`, `manager`, and `node` modules (default on)
- Added support for user-provided Tokio runtime in live module (#3918), thanks @filipmacek
- Added continuous futures support for bar requests and subscriptions (#3921), thanks @faysou
- Improved `nautilus-live/defi` to no longer pull `LiveNode` orchestration deps
- Improved CI uv cache via `setup-uv` auto mode to skip GHA uploads on self-hosted runners (#3933), thanks @sunlei
- Improved CI cache hygiene on self-hosted runners with uv prune, prek auto-gate, and footprint summary
- Migrated `WebSocketClient` onto the `WsTransport` trait, decoupling reconnect/auth from tungstenite types (Rust)
- Changed Polymarket `PolymarketQuote.best_bid`/`best_ask` to optional, matching the Rust `Option<String>` schema
- Ported Interactive Brokers Rust historical bar replay with Python parity fixes (#3892), thanks @faysou
- Standardized adapter example manifests and trading deps (#3891), thanks @sunlei
- Standardized margin emission convention across live derivatives adapters to use currency-keyed `MarginBalance` entries
- Refactored `reconciliation` module into `types`, `ids`, `positions`, and `orders` submodules (Rust)
- Refactored Binance Futures user data stream dispatch and listen key recovery into dedicated modules (Rust)
- Refactored Binance Futures value conversions into a new `futures::conversions` module (Rust)
- Replaced `AHashMap`/`AHashSet` with `IndexMap`/`IndexSet` in `ExecutionManager` for deterministic ordering in simulations (Rust)
- Refined `nautilus-system` to optional in adapter crates (gated by `python`); default builds drop heavy transitive deps
- Refined DST convention hook to enforce `IndexMap` in `OrderMatchingEngine`
- Refined make cargo-test to not include binaries for test harness builds (#3828), thanks @faysou
- Refined Interactive Brokers combo fill average price calculation (#3834), thanks @faysou
- Refined Kraken WebSocket execution dispatch to emit typed events for tracked orders via per-product modules
- Refined Kraken Spot WS auth via `AuthTracker` with `is_authenticated`/`wait_until_authenticated` Python APIs
- Optimized Hyperliquid L1 signing by caching `PrivateKeySigner` and EIP-712 domain (#3851)
- Optimized `ClientOrderId` generation with cached prefix buffer (#3935), thanks @sunlei
- Optimized `OrderListId` and `PositionId` generation with cached prefix buffers (Rust)
- Upgraded Rust (MSRV) to 1.95.0
- Upgraded Cap'n Proto to v1.4.0
- Upgraded `alloy` crate to v2.0.1
- Upgraded `capnp` crate to v0.25.4 (regenerated schemas with 4-space indents and version headers)
- Upgraded `databento` crate to v0.48.0
- Upgraded `datafusion` crate to v53.1.0
- Upgraded `msgspec` to v0.21.1
- Upgraded `pyarrow` to v24.0.0
- Upgraded `tokio` crate to v1.52.1

### Documentation Updates
- Added Polymarket Python and Rust adapter config tables and updated rate limits
- Added ID determinism invariant to the reconciliation live and execution concept guides
- Added Trade ID derivation sections to Polymarket, Databento, BitMEX, and Tardis integration guides
- Added Trade ID derivation section to the backtesting concept guide
- Added "Equity and mark-to-market" section to the portfolio concept guide
- Added ADL / liquidation handling sections to the Binance, Bybit, OKX, BitMEX, Hyperliquid, Deribit, dYdX guides
- Added reconciliation reports section to the execution concept guide
- Refined docs to follow style guide for symbols and filler words (#3830), thanks @JKDasondee
- Refined Interactive Brokers documentation regarding UTC timestamps (#3826), thanks @faysou
- Refined dYdX integration guide config tables to match the Python API (`environment`, `subaccount`, `base_url_grpc`)
- Updated Hyperliquid integration guide with funding history, depth10, subscribe_params, and slippage
- Updated the configuration concept guide to define unknown-field rejection as the config standard in Python and Rust

### Deprecations
- Deprecated `demo`/`testnet` bools on `BybitDataClientConfig`/`BybitExecClientConfig` - use `environment`
- Deprecated `is_demo` on `OKXDataClientConfig`/`OKXExecClientConfig` - use `environment`
- Deprecated `testnet` on `HyperliquidDataClientConfig`/`HyperliquidExecClientConfig` - use `environment`
- Deprecated `is_testnet` on `DeribitDataClientConfig`/`DeribitExecClientConfig` - use `environment`
- Deprecated `is_testnet` on `DydxDataClientConfig`/`DydxExecClientConfig` - use `environment`
- Deprecated `testnet` on `BitmexDataClientConfig`/`BitmexExecClientConfig` - use `environment`

---

# NautilusTrader 1.225.0 Beta

Released on 6th April 2026 (UTC).

### Enhancements
- Added option chains and greeks in Rust (#3637), thanks @filipmacek
- Added option chains and greeks in Python (#3677), thanks @filipmacek
- Added cached futures-spread support to `GreeksCalculator` (#3792), thanks @faysou
- Added custom data registration, persistence, and routing in Rust (#3542), thanks @faysou
- Added `nautilus_actor!` macro in `nautilus_common` for `Deref`/`DerefMut` boilerplate on actor types (Rust)
- Added `nautilus_strategy!` macro in `nautilus_trading` for `Deref`/`DerefMut` and `Strategy` trait boilerplate on strategy types, with optional block for hook overrides (Rust)
- Added `cache.orders_active_local(...)` function in Rust (#3716), thanks @Javdu10
- Added `interval` field to `FundingRateUpdate` (#3694), thanks @dxwil
- Added `BookImbalanceActor` example actor for order book quoted volume imbalance in Rust
- Added `ExecTesterConfig.test_reject_post_only` implicitly setting `post_only` on orders without requiring `use_post_only` (Python and Rust)
- Added `TieredTickScheme` and `TickScheme::Tiered` for price-dependent tick sizes (Rust)
- Added `TokenizedAsset` instrument type with configurable `asset_class` field for tokenized equities, ETFs, commodities, and other real-world assets
- Added Betfair backtest example streaming raw `.gz` data through `BacktestEngine` (Rust)
- Added Binance `decode_binance_spot_client_order_id` and `decode_binance_futures_client_order_id` utility functions for decoding Link & Trade encoded `clientOrderId` values from raw Binance API responses
- Added Binance Futures `subscribe_funding_rates` and `unsubscribe_funding_rates` with `FundingRateUpdate` emission via the mark price stream (Rust)
- Added Binance Futures exchange-generated order handling for liquidation, ADL, and settlement fills with client order ID prefix detection and `FillReport`/`OrderStatusReport` emission (Rust)
- Added Binance Futures `use_position_ids` config for hedging position IDs derived from instrument and position side on exchange-generated fills (Rust)
- Added Binance Futures `default_taker_fee` config with commission fallback estimation for exchange-generated fills when venue omits commission fields (Rust, USD-M only)
- Added Binance `NewAdl`, `NewInsurance`, and `PendingNew` variants to `BinanceOrderStatus` (Rust)
- Added Binance `Rpi` time-in-force, `PreSettle`/`Settling`/`Close` contract statuses, `None`/`Decrement`/`Transfer` STP modes, and income type variants (Rust)
- Added Binance instrument status polling in Rust
- Added Arrow schema support for `BinanceBar` and `BinanceFuturesMarkPriceUpdate` (#3749), thanks @twitu
- Added Binance Futures `close_position` parameter for algo stop orders to close an entire position at trigger price (Python and Rust) (#3751), thanks for reporting @dodge-basic
- Added Bybit native TP/SL params for order placement (#3754), thanks @jindrichsirucek
- Added Bybit instrument status polling and subscription (#3738), thanks @filipmacek
- Added Bybit options trade subscriptions using `baseCoin` topic with per-instrument filtering
- Added Bybit option instrument fee rate population from `/v5/account/fee-rate`
- Added Bybit `submit_order_list` via WebSocket batch API with TP/SL support and HTTP demo fallback (Rust)
- Added Bybit `query_order` via HTTP with open order and history fallback (Rust)
- Added Databento Arrow serialization for imbalance and statistics (#3689), thanks for reporting @GianC0
- Added Deribit `LimitIfTouched` and `MarketIfTouched` order type support (`take_limit`/`take_market`)
- Added Hyperliquid agent wallet support (#3668), thanks @oh92
- Added Hyperliquid product type config for live clients (#3783), thanks @lisiyuan656
- Added Kraken FOK, `LimitIfTouched` orders, and batch submit
- Added Kraken tokenized equity (xStocks) support via `aclass_base=tokenized_asset` with automatic dual-fetch on instrument loading (#3455), thanks for reporting @jilongjia
- Added Kraken `request_book_snapshot` for spot and futures via HTTP depth endpoints
- Added Kraken `request_funding_rates` for futures with client-side start/end/limit filtering
- Added Kraken `subscribe_instrument_status` for spot and futures (polling-based detection)
- Added Kraken spot trailing stop and trailing stop limit order submission with `trailing_offset` and `limit_offset` fields
- Added Kraken spot `trigger` parameter for conditional orders (`last` or `index` price reference)
- Added Kraken spot quote quantity orders via `viqc` order flag
- Added Kraken spot iceberg orders via `displayvol` parameter
- Added OKX `submit_order_list` via WebSocket batch endpoint for regular GTC orders
- Added OKX support for bracket order submission with attached TP/SL (#3701), thanks @Nickonomic
- Added OKX `subscribe_option_greeks` for venue-provided Greeks via the `opt-summary` WebSocket channel
- Added OKX configurable `ws_auth_timeout_secs` for WebSocket authentication (#3727), thanks for reporting @Stamppot82
- Added OKX `fwdPx` (forward price) to `OKXOptionSummaryMsg` and mapped to `underlying_price` on `OptionGreeks` for ATM tracking
- Added OKX `request_orderbook_snapshot` and `request_funding_rates` to Python data client via PyO3 bindings
- Added OKX options trading execution with limit orders, `px_usd`/`px_vol` pricing modes, `OpFok` order type, and `MarketToLimit`/conditional order rejection
- Added OKX options position-level Black-Scholes Greeks (`delta_bs`, `gamma_bs`, `theta_bs`, `vega_bs`) to position data
- Added OKX `determine_order_type_with_alt` for correct order type classification when options use alternative pricing fields
- Added `DeltaNeutralVol` strategy strangle entry via `px_vol` limit orders with configurable IV offset, time-in-force, and cache-based re-entry guard
- Added OKX missing WebSocket message fields across all channel structs
- Added Polymarket instrument provider and filters in Rust (#3708), thanks @filipmacek
- Added Polymarket strategy-driven data subscriptions (#3806), thanks @Javdu10
- Added Tardis `MarkPriceUpdate` and `IndexPriceUpdate` parsing from `derivative_ticker` messages in Rust
- Added Tardis `DerivativeTickerCache` for deduplicating unchanged funding rate, mark price, and index price updates
- Added Tardis `TardisDataType` enum for normalized Tardis Machine data type identifiers
- Added Tardis live streaming support via `stream_options` config with automatic reconnection and exponential backoff
- Added Tardis raw provider metadata to `Instrument.info` (#3730), thanks for reporting @volemont

### Breaking Changes
- Removed deprecated `convert_quote_qty_to_base` from `ExecEngineConfig` and `LiveExecEngineConfig`; adapters now handle quote-to-base conversion directly
- Removed `TARDIS_BASE_URL` constant from `nautilus_tardis::http` - use `nautilus_tardis::common::urls::TARDIS_HTTP_BASE_URL`
- Removed Hyperliquid `revoke_hyperliquid_builder_fee` function and builder fee revoke scripts
- Removed `DatabentoLiveClient.key` property (Python)
- Renamed `OrderEvent.kind()` to `type_name()` in Rust
- Renamed instrument `type_str` PyO3 getter to `type_name`
- Renamed `DatabentoHistoricalClient.key` property to `api_key` (Python)
- Renamed `ParquetDataCatalogV2` to `ParquetDataCatalog` and `StreamingFeatherWriterV2` to `StreamingFeatherWriter` (PyO3 persistence classes)
- Changed Tardis HTTP client from `reqwest::Client` to `nautilus_network::http::HttpClient` with rate limiting
- Changed `ExecutionEngine.register_client` to error when a venue is already routed to another client (Rust)
- Changed `ExecutionEngine.register_venue_routing` to error when re-routing a venue to a different client (Rust)
- Changed collection-cloning PyO3 getters to methods: `Position.events()`, `adjustments()`, `client_order_ids()`, `venue_order_ids()`, `trade_ids()`; and `events()` on all order types
- Changed config structs to use `bon::Builder` defaults as single source of truth; `Default` impls now delegate to `Self::builder().build()`
- Changed config fields that always had a sensible default from `Option<T>` to plain `T` with `#[builder(default)]` across all adapter, live, and engine configs (Rust)
- Changed `Option<T>` fields retained only where `None` carries distinct meaning (feature disabled, unbounded, etc.)

### Security
- Hardened Docker Compose to bind all ports to localhost and add `no-new-privileges` to all services
- Hardened CI egress policy to block by default and fall back to `audit` mode for fork pull requests
- Upgraded all `nautilustrader.io` URLs from HTTP to HTTPS (#3686), thanks @04cb
- Documented `aws-lc-rs` non-FIPS mode rationale (FIPS 140-3 module requires Go toolchain)

### Fixes
- Fixed `OrderBook` L1 stale event mutation corrupting bid/ask (#3790), thanks for reporting @linimin
- Fixed position index blob pollution in `update_position` (#3791), thanks @YeeTsai
- Fixed `purge_order` `KeyError` for position/exec_algorithm index access (#3799)
- Fixed strategy receiving historical events during startup reconciliation (#3793), thanks @filipmacek
- Fixed `Trader::add_exec_algorithm` not registering the `{id}.execute` msgbus endpoint, causing orders with `exec_algorithm_id` to be silently dropped
- Fixed `Trader::clear_exec_algorithms` and `dispose_components` not deregistering `{id}.execute` msgbus endpoints for removed algorithms
- Fixed `TopicRouter` stale index cache panic when unsubscribing one pattern invalidated indices for unrelated cached topics (#3755), thanks for reporting @Javdu10
- Fixed `PRICE_UNDEF` panic in `OrderBookDelta.to_pyo3_list` Cython conversion (#3697), thanks @zshuang15
- Fixed `ExecutionEngine` silently dropping `SubmitOrder` and `SubmitOrderList` commands when no execution client can be resolved; now emits `OrderDenied` (Rust)
- Fixed `RiskEngine` RefCell re-entrancy panic on order denial (#3680), thanks @husariancom
- Fixed reconciliation when trigger_price is set for non-conditional orders (#3673), thanks @husariancom
- Fixed `subscribe_instruments` using exact topic instead of wildcard pattern, causing venue-level subscriptions to miss per-instrument publishes from `DataEngine` (Rust)
- Fixed spurious "Timer replaced" warnings for expired timers in `LiveClock` and `TestClock` (#3690), thanks @HaakonFlaaronning
- Fixed time bar historical event deferral (#3698), thanks @faysou
- Fixed `DataActor` and `Strategy` timer callbacks in live mode silently lost on shared clock
- Fixed `DataActor::handle_time_event` missing `not_running()` state guard
- Fixed `SimulatedExchange` account balance adjustment mutation (#3704), thanks for reporting @thaning0
- Fixed analyzer and tearsheet returns to prefer portfolio-level daily returns when they can be derived from account balances
- Fixed backtest analyzer to include position snapshots in Rust (#3710), thanks @necofx
- Fixed `make_dict_serializer` incompatible with instance-method `to_dict` for `@customdataclass` types (#3722), thanks for reporting @Lacleman-trading
- Fixed Sandbox `RefCell` re-entrancy panic when submitting orders through `ExecutionEngine` in async runner (#3732), thanks for reporting @linimin
- Fixed triggered stop orders remaining in matching core after full fill, causing repeated duplicate fill log messages (#3741), thanks for reporting @linimin
- Fixed matching engine `L1_MBP` stale bid/ask when backtesting with trade-only data (Rust and Cython)
- Fixed matching engine GTD order expiry running after fills, allowing expired orders to fill before being expired
- Fixed `Order::calculate_overfill` emitting false `Quantity` saturation warnings during normal partial fills (#3746), thanks for reporting @linimin
- Fixed Sandbox reconciliation missing `account_id` (#3705), thanks for reporting @eliotOrderson
- Fixed Rust `Portfolio` account-scoped `net_exposure`, `net_exposures`, and balance updates in multi-account mode
- Fixed `RefCell` borrow conflict in `Portfolio::initialize_orders` (#3787), thanks @filipmacek
- Fixed reported `MarginAccount` updates dropping initial and maintenance margins (#3725), thanks for reporting @marco-rigoni
- Fixed option chains emitting data after expiry (#3735), thanks @filipmacek
- Fixed `BettingInstrument.selection_handicap` PyO3 name
- Fixed adapter `query_account` panic from `block_on` inside async runtime across all adapters (Rust)
- Fixed Betfair order modify `Quantity` serialization for partial cancel size reduction
- Fixed Binance trailing stop params and testnet URLs (#3778), thanks @eliotOrderson
- Fixed Binance Spot SBE schema version mismatch after Binance upgraded to schema 3:3 (released 2026-03-25)
- Fixed Binance algo order update (#3665), thanks @qu1zzyboy
- Fixed Binance SBE price/quantity precision derivation (#3670), thanks @husariancom
- Fixed Binance Futures `set_futures_hedge_mode` sending GET instead of POST to `positionSide/dual` endpoint (#3745), thanks for reporting @dodge-basic
- Fixed Binance order update silently dropped when instrument not cached (#3775), thanks for reporting @M-at-ti-a
- Fixed Binance Futures `OrderStatusReport` missing `avg_px` from WS order updates (Python)
- Fixed Binance Spot post-only (`LIMIT_MAKER`) rejection not setting `due_post_only` on `OrderRejected` events (Python and Rust)
- Fixed Binance Rust WS trading API not decoding SBE error responses, losing error codes on rejection
- Fixed Binance Rust WS trading request-response race condition where fast rejections arrived before pending request registration
- Fixed Binance Rust WS trading `OrderRejected` DashMap deadlock when `cleanup_terminal` ran while holding a read guard
- Fixed Binance Spot Rust `connect()` not waiting for WS session authentication before signaling connected
- Fixed Binance Futures account state parsing failing on empty string balances from inactive accounts
- Fixed Bybit demo exec client failing with error 10001 when `/v5/account/fee-rate` is unavailable (#3742), thanks for reporting @jindrichsirucek
- Fixed Bybit HTTP client not retrying on 429 rate limit responses
- Fixed Bybit HTTP cancellation token not resettable after `disconnect()`, causing REST calls to short-circuit on reconnect
- Fixed Bybit WebSocket subscription ACKs confirming all pending topics instead of the acknowledged topic (via `req_id` correlation)
- Fixed Bybit WebSocket failed subscription ACKs (success=false) not triggering `mark_failure` recovery path
- Fixed Bybit spot market orders ignoring `is_quote_quantity` on the order, causing all spot market buys to default to quote currency quantity via the Bybit API
- Fixed Bybit demo mode `submit_order` ignoring `is_leverage` param, hardcoding `false` instead of reading from order params
- Fixed Bybit `trigger_type` ignored on conditional orders, always submitting as `LastPrice` (#3794), thanks for reporting @marco-rigoni
- Fixed Bybit TP/SL conditional orders misclassified as plain Market/Limit during reconciliation
- Fixed Bybit bulk order status reports silently missing conditional (stop/MIT) orders
- Fixed Bybit account state free balance underflowing when locked margin exceeds wallet total during liquidation
- Fixed Databento price precision truncation for fractional tick sizes (#3696), thanks @pandashark
- Fixed Deribit stop order submission missing `trigger_price` and `trigger` fields in Python exec client (#3794), thanks for reporting @marco-rigoni
- Fixed Deribit cancel event lost during WebSocket reconnection gap when `user.orders` subscription update never arrives
- Fixed Deribit duplicate `OrderCanceled` events when cancel RPC response and `user.orders` subscription both emit
- Fixed Deribit `GenerateOrderStatusReport` unable to find closed orders when only `client_order_id` is provided
- Fixed Deribit `next_8_utc` GTD expiry calculation panicking on edge-case timestamps outside nanosecond range
- Fixed Deribit historical trade pagination dropping trades when >1000 share a millisecond boundary
- Fixed Deribit late-listed instruments not propagating to HTTP and WebSocket handler caches
- Fixed Deribit `request_book_snapshot` silently using default 8/8 precision when instrument not in cache
- Fixed Deribit `request_bars` ignoring `limit` parameter
- Fixed Deribit `request_forward_prices` ignoring request `client_id` override
- Fixed Deribit `reset()` leaking stream tasks by replacing cancellation token without canceling the old one
- Fixed Deribit `send_auth_request` silently dropping serialization and channel send errors
- Fixed Deribit `send_subscribe`/`send_unsubscribe` leaving subscription state wedged on command send failure
- Fixed Deribit `VenueOrderId` comparison via unnecessary string conversion in fill report filtering
- Fixed Deribit `OrderSide` conversion using fragile string round-trip instead of `order_side_to_pyo3` in `_submit_order` and `_submit_order_list`
- Fixed Deribit WebSocket `connect()` not clearing subscription state for manual disconnect/reconnect cycles
- Fixed dYdX WebSocket account state parsing failing on empty string balances from zero-equity accounts
- Fixed dYdX WebSocket handler repeatedly emitting `NewInstrumentDiscovered` for uncached instruments on every `v4_markets` update
- Fixed Hyperliquid `_submit_order_list` passing raw Cython orders to Rust, causing `TypeError` on bracket/batch orders (#3763), thanks for reporting @jindrichsirucek
- Fixed Hyperliquid `_modify_order` `OrderSide` conversion using fragile string round-trip instead of `order_side_to_pyo3`
- Fixed Hyperliquid vault orders rejected with "Builder fee has not been approved" when `vault_address` is configured (#3762), thanks for reporting @chester0
- Fixed Interactive Brokers docs `request_ticks` API and add contract example (#3699), thanks @faysou
- Fixed Interactive Brokers live-session synchronization and reconciliation (#3715), thanks @faysou
- Fixed Interactive Brokers shared historical request dedup for concurrent warmup (#3719), thanks @Johnkhk
- Fixed Interactive Brokers historical bar subscriptions not restored after daily gateway restart (#3733), thanks for reporting @bomber555
- Fixed Interactive Brokers inactive order status handling to prevent silent dropping (#3723), thanks @pandashark
- Fixed Interactive Brokers trailing stop order field parsing during reconciliation and open-order updates (#3771), thanks @faysou
- Fixed Interactive Brokers spread instrument not found on restart reconciliation (#3753), thanks @davidsblom
- Fixed Interactive Brokers adapter not reconnecting on error 326 during gateway restart (#3796), thanks @Johnkhk
- Fixed Kraken post-only order rejection not setting `due_post_only` on `OrderRejected` events (Spot and Futures)
- Fixed OKX option conditional order rejection emitting `OrderSubmitted` before `anyhow::bail!`, leaving orders stuck in `Submitted` state
- Fixed OKX `MarketToLimit` orders not rejected for options in HTTP and WebSocket clients
- Fixed OKX `determine_order_type` classifying IV/USD-priced option IOC orders as `Market` when primary `px` field is empty
- Fixed OKX BboTbt quote parsing spamming errors on empty bid/ask arrays for illiquid options by adding `QuoteCache` for partial quote merging
- Fixed OKX `_subscribe_instrument_status` raising `NotImplementedError` instead of being a no-op (status detected via polling)
- Fixed OKX `batch_cancel_all_orders` and `batch_cancel_orders` not emitting `OrderCancelRejected` events for regular (non-algo) batch cancel failures
- Fixed OKX `batch_submit_orders` not removing `order_identities` from dispatch state on batch submit failure
- Fixed OKX business WebSocket requiring API credentials for public-only candle data
- Fixed OKX `parse_fill_report` erroring on zero incremental fill quantity during reconnect replay instead of skipping gracefully
- Fixed OKX `request_position_status_reports` querying positions API for Spot/Margin instruments (unsupported by endpoint)
- Fixed OKX `cancel_all_orders` and `batch_cancel_orders` not seeding `order_identities` for reconciliation-loaded orders
- Fixed OKX `pending_orders`, `pending_cancels`, and `pending_amends` maps leaking entries on WebSocket send failure
- Fixed OKX duplicate fills after WebSocket reconnect when replayed messages have the same `trade_id`
- Fixed OKX HTTP algo order helpers ignoring per-item `sCode`, treating venue rejections as success
- Fixed OKX batch algo cancel not emitting `OrderCancelRejected` events for per-item or batch-level failures
- Fixed OKX spot margin short position quantity exceeding `size_precision` from quote-to-base division
- Fixed OKX `parse_rfc3339_timestamp` silently wrapping negative `i64` nanoseconds to garbage `u64`
- Fixed OKX `update_fee_fill_caches` diverging from shared `parse_fee_currency` (missing non-zero fee warning)
- Fixed OKX duplicate fill early return skipping terminal cleanup for `order_identities` and `order_state_cache`
- Fixed OKX position status reports incorrectly filtered by `start`/`end` time, dropping unchanged open positions
- Fixed OKX `connect()` not passing `instrument_families` for OPTION instrument requests (HTTP 400 from OKX API)
- Fixed OKX `base_url_ws` ignored for private and business WebSocket channels (#3727), thanks for reporting @Stamppot82
- Fixed OKX exec client crash on empty account when OKX returns empty strings for numeric balance fields (#3772), thanks for reporting @ProfitChef
- Fixed Polymarket WebSocket initial vs incremental subscribe (#3717), thanks @Javdu10
- Fixed Polymarket cancel request silently dropped when `venue_order_id` not yet available, causing order to remain open until next reconciliation (Python and Rust)
- Fixed Polymarket market BUY quote-to-base quantity calculation using worst crossing price instead of per-level accumulation (#3747), thanks @filipmacek
- Fixed Polymarket FOK orders stuck in accepted state when WS terminal status update is missed; deferred REST status check resolves after 5s
- Fixed Polymarket fee rate cache serving stale values indefinitely; added 5-minute TTL with graceful fallback on refresh failure
- Fixed Polymarket `calculate_market_price` not bailing when all book levels have zero price or size
- Fixed Polymarket `created_at` timestamp conversion (#3785), thanks @filipmacek
- Fixed Polymarket `ts_init` timestamps on reports and reconciliation (#3786), thanks @filipmacek
- Fixed Polymarket position reconciliation dust cycling by filtering sub-threshold positions and implementing Data API position reports (#3774), thanks @filipmacek
- Fixed Polymarket duplicate inferred fill panic when order update races trade (#3770), thanks for reporting @Javdu10
- Fixed Polymarket `query_order` panic from `block_on` inside async runtime (#3803), thanks for reporting @Javdu10
- Fixed Polymarket order stuck in non-terminal state when fills race with cancel (#3797), thanks for reporting @Javdu10
- Fixed Tardis data client CTRL+C not responding due to signal starvation in `LiveNode` event loop
- Fixed Tardis data client `stop()`/`disconnect()` lifecycle leaving tasks alive or `is_connected` stale
- Fixed Tardis data client `derivative_ticker` not streaming unless manually added to `data_types`

### Internal Improvements
- Added `SpreadQuoteAggregator` (#3698), thanks @faysou
- Added `Params` and `dict` field support for `#[custom_data]` and `@customdataclass` persistence (#3765), thanks @faysou
- Added `BINANCE_GTX_ORDER_REJECT_CODE` and `BINANCE_SPOT_POST_ONLY_REJECT_MSG` constants for reliable post-only rejection detection in Rust
- Added `batch_submit_limit_pair` to `ExecTesterConfig` for order list testing
- Added Python strategy support to v2 `LiveNode` with `add_strategy_from_config`
- Added Python exec algorithm support to v2 `LiveNode` with `add_exec_algorithm_from_config`
- Added `LiveNode` integration tests for actor, strategy, and exec algorithm registration
- Added `LiveNode::add_exec_algorithm` for registering execution algorithms on the Rust live trading node
- Added `LiveNode` stop-handle timeout test for shutdown reliability
- Added `ExecutionEngine` runtime external order creation from `OrderStatusReport` for exchange-generated orders (liquidation, ADL, settlement) not previously in cache (Rust)
- Added `add_exec_algorithm_from_config` PyO3 binding on `LiveNode` with `ImportableExecAlgorithmConfig`
- Added `msgbus::has_endpoint` for endpoint existence checks
- Added backtest margin models, `FXRolloverInterestModule`, `PerContractFeeModel`, and `SimulationModule` trait in Rust
- Added `subscribe_option_greeks` support to `DataTester` in Rust
- Added `WebSocketClient.notify_closed()` for stream-mode callers to signal reader EOF to the controller
- Added pending cancel/update to event emitter in Rust (#3739), thanks @Javdu10
- Added `LimitIfTouched`, `MarketToLimit`, `TrailingStopMarket`, and `TrailingStopLimit` to `transform_order_to_pyo3` Cython-to-PyO3 order converter
- Added PyO3 type assertions to adapter submit-order tests (Hyperliquid, Bybit, Kraken, Architect AX) to catch Cython/PyO3 type boundary regressions
- Added Binance missing `BinanceFilterType` variants and `RawRequests` rate limit type for complete API enum coverage (Rust)
- Added Binance unit tests for liquidation, ADL, settlement, and insurance fill parsing with `is_exchange_generated` detection (Rust)
- Added Binance parametrized tests for `resolve_commission` fallback and `make_venue_position_id` (Rust)
- Added Binance Futures priceMatch (BBO) order support (Rust)
- Added Bybit `BybitWsFrame` enum separating wire-level deserialization from public `BybitWsMessage` API per adapter spec pattern
- Added Bybit frame classification and subscription correlation test coverage (25 handler tests)
- Added Databento feed handler integration tests with mock LSG server
- Added Databento MBO buffering unit tests and proptests
- Added OKX `QuoteCache` integration and option greeks subscription lifecycle tests
- Added OKX reconciliation pagination cap warnings when fetches hit the maximum page limit
- Added OKX trade-level fill dedup via `emitted_trades` DashSet with atomic insert for cross-stream safety
- Added OKX `AlgoCancelContext` and `dispatch_algo_cancels` to centralize algo cancel partitioning and rejection handling
- Added OKX options fill fields (`fill_px_vol`, `fill_px_usd`, `fill_fwd_px`) and order pricing fields (`px_usd`, `px_vol`) to WebSocket and HTTP models
- Added OKX execution client integration tests for trade dedup, algo cancel rejections, batch cancel failures, and concurrent dedup
- Added OKX HTTP mock test for `place_algo_order` `sCode` rejection path
- Added OKX `OKXPriceType`, `OKXSettlementState`, `OKXQuickMarginType` enums for type-safe field deserialization
- Added Tardis HTTP and WebSocket mock server integration tests
- Replaced Binance `WsDispatchState` `DashSet` dedup with `FifoCache` from `nautilus_common` for bounded FIFO eviction with proper `remove()` cleanup
- Replaced Bybit topic string constants with `BybitWsPublicChannel` and `BybitWsPrivateChannel` enum references
- Replaced `AtomicMap` and `AtomicSet` type aliases with newtypes wrapping `ArcSwap` for ergonomic read-heavy concurrent collections
- Replaced `DashMap`/`DashSet` with `AtomicMap`/`AtomicSet` for subscription tracking sets, instrument caches, and bar type caches across all adapters
- Refactored computation of greeks (#3691), thanks @faysou
- Refactored `DataEngine` instrument subscribers to message bus pattern, enabling execution clients to receive live instrument updates via `on_instrument` without polling (#3766), thanks @filipmacek
- Refactored data and execution client startup into two phases with a data event drain between them (#3773), thanks @filipmacek
- Refactored Deribit trade pagination into `TradePaginator` with dedup and cursor logic shared across public trades and fill reports
- Refactored Polymarket HTTP client and improved outcome enum (#3702), thanks @filipmacek
- Refactored Tardis adapter module organization to align with adapter spec (`common/`, `machine/cache.rs`)
- Refactored Tardis `TardisDataClient` with `Credential::resolve()`, centralized URL resolution, and `AHashMap`
- Regenerated Binance Spot SBE codecs from schema 3:3 XML using Real Logic SBE tool v1.37.1
- Moved cache purge timers to base `ExecutionEngine` in Python
- Improved socket clients reconnect and shutdown reliability
- Improved `LiveNode` event loop to use biased `select!` with pinned `ctrl_c` for reliable signal handling
- Improved Binance Spot SBE HTTP parsers to use `block_length` from the message header for end-of-block skip, making decoders forward-compatible with future schema additions
- Improved Databento live price precision handling with maps populated from instrument definitions
- Improved Polymarket Rust adapter (#3726), thanks @filipmacek
- Improved Polymarket execution client (#3734), thanks @filipmacek
- Improved Polymarket adapter in Rust (#3760), thanks @filipmacek
- Refined `TimeEventHandler` ordering and fixed spread quote timestamps (#3764), thanks @faysou
- Refined `SpreadQuoteAggregator` transition from historical to live mode (#3759), thanks @faysou
- Refined handling of instruments in catalog (#3761), thanks @faysou
- Refined `AtomicTime` mode switching and datetime panics
- Refined base catalog interface (#3703), thanks @faysou
- Refined IB option symbols to be OCC compliant (#3731), thanks @faysou
- Standardized `type_name()` across order events and instruments
- Wired `ExecutionManager` into live event loop with full inflight lifecycle (Rust) (#3798), thanks @filipmacek
- Optimized network client performance and add benchmarks
- Upgraded Interactive Brokers `ibapi` to 10.45 (#3804)
- Upgraded Rust (MSRV) to 1.94.1
- Upgraded `capnp` and `capnpc` crates to v0.25.3 (regenerated schemas with 4-space indents and version headers)
- Upgraded `databento` crate to v0.45.0
- Upgraded `datafusion` crate to v53.0.0
- Upgraded `pyo3` crate to v0.28.3
- Upgraded `redis` crate to v1.2.0
- Upgraded `tokio` crate to v1.51.0
- Upgraded `tokio-tungstenite` crate to v0.29.0

### Documentation
- Added Rust tutorial for Betfair book imbalance backtest with `DataActor` walkthrough
- Added Options concept guide with chain architecture, subscription API, strike filtering, and snapshot modes
- Added Greeks concept guide covering venue-provided and local calculator paths
- Added end-to-end data flow and execution flow sequence diagrams to architecture concepts
- Added Events concept guide with event catalog, handler dispatch, and fill-to-position chain
- Added Rust concept guide with capability matrix, project setup, and feature flags
- Added `how_to/configure_live_trading.md` extracted from `concepts/live.md` configuration content
- Added adapter developer guide sections for WS unit tests, close/stream patterns, and split-client architecture
- Added adapter developer guide sections for symbol normalization, status diffing, task management, data event emission, and AuthTracker
- Added adapter developer guide section on configuration best practices: builder defaults, `T` vs `Option<T>` rules, `Default` delegation pattern
- Added adapter developer guide section on `block_on` safety rules and `spawn_task` usage in sync trait methods
- Added OKX options trading section to integration guide with pricing modes, order types, restrictions, and configuration
- Added Group 10 (options trading) to execution testing spec with venue-agnostic test cases
- Added `DeltaNeutralVol` README updates for strangle entry flow, config fields, and usage examples
- Added Binance Link & Trade `clientOrderId` decoding section with usage examples to integration docs
- Added Bybit options support matrix and trading limitations to integration docs
- Added OKX to adapter support tables in Options and Greeks concept guides
- Added option greeks test cases (TC-D62, TC-D63) with config examples to the data testing spec
- Added test style guidance against log capture assertions in developer testing guide
- Rewrote Live Trading concept guide for accuracy (reconciliation, periodic timers, lookback windows)
- Rewrote Custom Data architecture docs for two-mode (Rust/Python) registration
- Improved Value Types concept guide with full arithmetic operator and unary operation docs
- Improved accuracy of Greeks and Options concept guides, thanks @faysou
- Improved `concepts/live.md` to focus on reconciliation explanation, linking to how-to for configuration
- Updated all API reference links to Sphinx HTML paths
- Migrated Python API reference from sphinx-markdown-builder to Sphinx HTML with Furo theme
- Fixed actors timer example referencing nonexistent `on_timer` and `on_alert` hooks

---

# NautilusTrader 1.224.0 Beta

Released on 3rd March 2026 (UTC).

### Enhancements
- Added matching engine L1 quote-based queue position tracking for backtests
- Added `fill_limit_inside_spread` to `FillModel` and `MatchingCore` for at-or-inside-spread limit fill control
- Added synthetic book support for binary markets (#3495), thanks @Javdu10
- Added `get_target_px_for_quantity` method on `OrderBook` (#3627), thanks @Javdu10
- Added Betfair batch submit and cancel order support
- Added BitMEX dead man's switch (cancelAllAfter) support (Rust and Python)
- Added BitMEX grid market maker example (Rust)
- Added BitMEX instrument status subscription support (Rust and Python)
- Added Bybit book snapshot and funding rate request support (Rust)
- Added Databento `skip_on_error` flag for `load_instruments` to skip unparsable definitions (#3657), thanks for reporting @davidsblom
- Added Deribit instrument status subscription support (Rust and Python)
- Added dYdX instrument status subscription support (Rust and Python)
- Added Hyperliquid order modify support (Rust and Python)
- Added OKX trailing stop market order support (Rust and Python)
- Added OKX algo order amend support (Rust and Python)
- Added OKX instrument status updates from WebSocket instruments channel (Rust)
- Added OKX index price subscriptions with base-pair remapping to derivatives (Rust)
- Added OKX book snapshot and funding rate request support (Rust)
- Removed Hyperliquid builder fee charges (builder-fee approval no longer required)

### Breaking Changes
- Removed Coinbase International (`COINBASE_INTX`) adapter, see RFC (#3555)
- Removed Binance `BINANCE_ED25519_*` env vars for Spot/Margin (use `BINANCE_API_KEY`/`BINANCE_API_SECRET`; Futures deprecated with warning)
- Removed Hyperliquid `builder_fee_refresh_mins` config option (builder fees no longer charged)
- Removed Polymarket `fetch_orderbook_history`, `load_orderbook_snapshots`, `fetch_price_history` and related methods (endpoints decommissioned, #3635)

### Security
- Added `pip-audit` to security audit pipeline
- Added Docker image cosign signing and SBOM generation
- Standardized credential zeroization across all adapters (`Ustr` replaced with `Box<str>` for API keys)
- Standardized secret redaction in `Debug` impls across all adapter credentials
- Updated `SECURITY.md` with expanded scope, reporting guidelines, and responsible disclosure policy
- Bumped all eligible GitHub Actions pinned SHAs to latest versions (2-week release policy)

### Fixes
- Fixed matching engine applying order book deltas for L1 books (#3615), thanks @maksym-mikheienko
- Fixed streaming backtest producing dummy bars past batch data exhaustion (#3628), thanks for reporting @cauta
- Fixed `OrderEmulator` trailing stop activation ignoring `LAST_PRICE` trigger type (#3629), thanks for reporting @HaakonFlaaronning
- Fixed `LiveExecEngine` position reconciliation infinite loop when venue reports flat (#3622), thanks for reporting @mrbaron3
- Fixed `CryptoOption` instrument pyo3 transform for (#3626), thanks @davidsblom
- Fixed `StreamingFeatherWriter` duplicate events from multiple message bus topics (#3625), thanks for reporting @fomotoshi
- Fixed `VolumeImbalanceBarAggregator` and `VolumeRunsBarAggregator` integer overflow for step >= 923 in high-precision mode (#3658), thanks for reporting @honvl
- Fixed `InstrumentProvider` `load_ids_async` loading all instruments instead of filtering to requested IDs (affected dYdX, Kraken, AX, Hyperliquid)
- Fixed Python WS callbacks running off asyncio event-loop thread in Rust adapters (#3653), thanks for reporting @camilorodegheri
- Fixed Binance Futures algo order serde field renames for WS and HTTP parsing (#3624), thanks for reporting @qu1zzyboy
- Fixed Binance silent HMAC fallback when using encrypted Ed25519 PEM keys (now warns)
- Fixed BinanceSymbol COIN-M perpetual symbol conversion (#3641), thanks @YeeTsai
- Fixed Binance algo order cancellation parsing (#3646), thanks @qu1zzyboy
- Fixed Binance Spot testnet WebSocket API URL (#3661), thanks @penguinwokrs
- Fixed Hyperliquid stop/trigger order price derivation (#3611), thanks for reporting @h-tsun3
- Fixed Hyperliquid price normalization and inner error detection (#3612), thanks for reporting @h-tsun3
- Fixed Interactive Brokers BarType/str comparison in get_historical_bars (#3616), thanks @powerseed
- Fixed Interactive Brokers historical bar processing crash (#3619), thanks @shzhng
- Fixed Interactive Brokers contract details parsing (#3638), thanks @davidsblom
- Fixed Kraken Spot and Futures execution clients not loading instruments during connect (#3644), thanks for reporting @husariancom
- Fixed Kraken Spot execution client HTTP client created without credentials (#3650), thanks for reporting @husariancom
- Fixed Kraken sequential `ClientOrderId` exceeding `cl_ord_id` 18-char free-text limit (#3651), thanks for reporting @husariancom
- Fixed Kraken missing account state registration during connect (#3652), thanks for reporting @husariancom
- Fixed Polymarket Gamma API `load_ids` path skipping sibling tokens (#3654), thanks for reporting @likenji
- Fixed Polymarket loader to use Data API trades instead of decommissioned orderbook/price history endpoints (#3635), thanks for reporting @JSai23
- Fixed Binance Spot testnet WebSocket API URL (legacy URL removed by Binance in May 2025) (#3660)
- Fixed pre-commit hooks portability for Windows (#3617), thanks for reporting @powerseed
- Fixed `LiveNode` startup `RefCell` panic when execution reports arrive during `connect()`
- Fixed dYdX new instrument discovery flooding logs with inactive/delisted markets
- Fixed dYdX fills and orders API requests missing required `marketType` parameter

### Internal Improvements
- Added catalog deduplication functionality (#3613), thanks @ms32035
- Extracted common SBE decoder to `nautilus-serialization` crate
- Implemented `BacktestNode` with catalog streaming in Rust
- Improved `OrderBookImbalance` example strategy
- Improved `BestPriceFillModel` to fill inside bid ask spread (#3428), thanks @faysou
- Standardized use of atomic clock across adapters
- Standardized adapter credentials handling and testing
- Refined build script for Windows (#3636), thanks @faysou
- Optimized matching engine `_seed_trade_consumption` to use range-bounded FFI queries for deep books
- Optimized backtest engine settle loop to avoid Python list allocation on idle ticks
- Optimized `MatchingCore.iterate` to avoid list concatenation on every call
- Upgraded `databento` crate to v0.42.0
- Upgraded `datafusion` crate to v52.2.0

### Documentation Updates
- Added AX Exchange gold perps book imbalance tutorial
- Added AX Exchange spot FX bars mean reversion tutorial
- Added BitMEX grid market maker tutorial
- Added adapter data and execution testing specifications
- Added order book concepts documentation
- Improved backtesting mermaid diagram and tutorial formatting

---

# NautilusTrader 1.223.0 Beta

Released on 21st February 2026 (UTC).

### Enhancements
- Added `bulk_read_batch_size` option to `CacheConfig` for batched Redis bulk reads, thanks @shzhng
- Added sandbox execution adapter in Rust
- Added multi-account execution support (#3194), thanks @faysou
- Added Nasdaq ITCH 5.0 parser
- Added grid market maker example strategy in Rust
- Added `OrderBookDeltas` historical request support (#3438), thanks @faysou
- Added `market_exit()` method for `Strategy` with configurable `market_exit_time_in_force` and `market_exit_reduce_only` options (supports venues requiring IOC for market orders)
- Added `manage_stop` config option to `StrategyConfig` for automatic market exit on stop
- Added matching engine `queue_position` tracking heuristic for backtests
- Added matching engine trade consumption seeding for L2/L3 book backtests
- Added tracing subscriber for external Rust library logs (`use_tracing=True` in `LoggingConfig`, filter with `RUST_LOG` env var)
- Added `use_market_order_acks` venue config option to generate `OrderAccepted` events for market orders before filling (mimics behavior of venues like Binance)
- Added `oto_trigger_mode` venue config option to control whether OTO child orders activate on partial fills (PARTIAL) or only after full fill (FULL) (default PARTIAL) (#3454), thanks @godnight10061
- Added `request_funding_rates` and `FundingRateUpdate` Arrow serialization (#3467), thanks @dxwil
- Added `optimize_file_loading` as BacktestDataConfig parameter (#3518), thanks @faysou
- Added `bulk_read_batch_size` option to `CacheConfig` for batched Redis bulk reads (#3535), thanks @shzhng
- Added `PerpetualContract` instrument for asset-class agnostic perpetual swaps
- Added Ichimoku Cloud indicator (#3552), thanks @faysou
- Added Betfair RCM parsing for TPD race data
- Added Betfair race stream subscription via `subscribe_race_data` config
- Added Betfair market version price protection for orders
- Added Betfair `BetfairOrderVoided` custom data type for VAR voids
- Added `BetfairOrderVoided` custom data type for VAR voids
- Added Binance `BinanceEnvironment` enum with `LIVE`, `TESTNET`, `DEMO` variants for explicit environment selection
- Added Binance `environment` config field to `BinanceDataClientConfig` and `BinanceExecClientConfig`
- Added Binance Demo environment support with `BINANCE_DEMO_API_KEY`/`BINANCE_DEMO_API_SECRET` env vars
- Added BitMEX trailing stop support
- Added BitMEX pegged order (BBO) support via params
- Added Bybit mark price subscriptions support
- Added Bybit index price subscriptions support
- Added Databento bulk subscription and historical request support (#3490), thanks @shzhng
- Added Databento support for conversion of OPRA venues (#3605), thanks @faysou
- Added Interactive Brokers subscribe index price functionality (#3514), thanks @Murph24
- Added Interactive Brokers `TotalCashValue` to account summary `info` dict, exposing actual cash balance (#3567), thanks @shzhng
- Added Interactive Brokers `request_timeout_secs` config to `InteractiveBrokersExecClientConfig` and consolidated all IB request timeouts into a single configurable value (#3602), thanks @shzhng
- Added OKX batch cancel support for conditional (algo) orders
- Added Polymarket data loader event-level API support (#3484), thanks @jsemldonado
- Added Polymarket `event_slug_builder` support (#3501), thanks @jsemldonado
- Added Polymarket batch order support (#3506), thanks @loafer-19
- Added Tardis data client with factory in Rust
- Improved tearsheet with dynamic Nautilus version and refined run info table (#3396), thanks @KaulSe

### Breaking Changes
- Removed dYdX v3 (legacy) Python adapter (the v3 exchange was decommissioned at end of 2024)
- Removed `dydx` optional install extra (the v4 Rust-backed adapter has no additional Python dependencies)
- Renamed `nautilus_trader.adapters.dydx_v4` module to `nautilus_trader.adapters.dydx` and standardized class names to `Dydx` prefix (e.g. `DydxDataClientConfig`, `DydxLiveDataClientFactory`)
- Removed dead `subscribe_order_book_snapshots` and `unsubscribe_order_book_snapshots` methods from `LiveMarketDataClient` (were never called by the data engine)
- Removed OKX URL environment variable overrides (`OKX_BASE_URL_HTTP`, `OKX_BASE_URL_WS_*`, `OKX_DEMO_BASE_URL_WS_*`); use config `base_url_*` fields instead
- Removed deprecated `get_ws_base_url` function from OKX Rust adapter; use `get_ws_base_url_private` or `get_ws_base_url_public` instead
- Removed `AddAssign`, `SubAssign`, `MulAssign` trait implementations from `Price`, `Quantity`, and `Money` types (Rust); use `x = x + y` instead of `x += y`
- Removed `add_assign` and `sub_assign` cdef methods from `Price`, `Quantity`, and `Money` types (Cython); use `x = x + y` instead
- Renamed `subscribed_order_book_snapshots` to `subscribed_order_book_depth` for consistency with data engine routing
- Removed `listen_key_ping_max_failures` from `BinanceExecClientConfig` (listenKey flow replaced by WebSocket API)
- Changed `Price`, `Quantity`, and `Money` arithmetic to use max precision instead of panicking on precision mismatch
- Changed `Quantity + Quantity`, `Quantity - Quantity`, `Price + Price`, `Price - Price`, `Money + Money`, and `Money - Money` Python operators to return the same type instead of `Decimal` (`Quantity - Quantity` raises `ValueError` if result would be negative)
- Changed `trade_execution` default from `False` to `True` for consistency with `bar_execution`; users who want to isolate execution to L1 book data only must now explicitly set `trade_execution=False`
- Changed price-protected market orders to no longer emit `OrderAccepted` by default; set `use_market_order_acks=True` to restore previous behavior
- Changed adapter implementations should now override `_subscribe_order_book_depth` and `_unsubscribe_order_book_depth` for `OrderBookDepth10` subscriptions
- Changed Binance execution clients now use WebSocket API authentication instead of listenKey REST API; both HMAC and Ed25519 keys are auto-detected from the `api_secret` format (no `key_type` config needed). Note: Futures with HMAC keys automatically fall back to REST listenKey management (Binance Futures WS API only supports Ed25519 for `session.logon`)
- Changed Binance execution clients now source credentials from the standard `BINANCE_API_KEY`/`BINANCE_API_SECRET` environment variables (or testnet equivalents)
- Changed Polymarket instrument provider config from `instrument_provider` to `instrument_config` on `PolymarketDataClientConfig` and `PolymarketExecClientConfig`; use `PolymarketInstrumentProviderConfig` instead of `InstrumentProviderConfig`

### Security
- Upgraded `arc-swap` to 1.8.1 fixing potential use-after-free in debt mechanism (memory ordering fix)
- Fixed `CVec::empty()` to use dangling pointer instead of null, avoiding undefined behavior in `Vec::from_raw_parts`
- Fixed credential and auth header leaks in trace logging
- Masked Binance listen keys in execution client logs
- Refactored supply chain security checks and update dependencies
- Improved TLS cert loading and socket suffix validation
- Hardened Postgres SQL and credential security

### Fixes
- Fixed matching engine liquidity consumption using cumulative book quantity
- Fixed matching engine liquidity consumption tracking for MAKER fills
- Fixed matching engine trade execution fills discarded with `liquidity_consumption`
- Fixed matching engine trade execution fill model and FOK/IOC handling
- Fixed matching engine trade ticks updating L1 book and triggering fills when `trade_execution=False`
- Fixed matching engine MAKER limit orders over-filling on L1 books when `liquidity_consumption=True`
- Fixed inverse instrument `base_currency` access across accounting
- Fixed logic and control flow bugs in core platform (#3585), thanks for reporting @pandashark
- Fixed cache reset and missing f-string prefixes (#3585), thanks for reporting @pandashark
- Fixed missing raise and divide-by-zero guards (#3598), thanks @pandashark
- Fixed account balance rounding mismatch for zero-precision currencies (#3579), thanks for reporting @penguinwokrs
- Fixed `Position` spot base currency commission sign (#3546), thanks for reporting @gaye746560359
- Fixed `Position` flat detection for floating-point edge cases
- Fixed `UnsubscribeInstrumentClose` message handler routing
- Fixed order cancel not releasing locked balance in backtest (#3525), thanks for reporting @dennisnissle
- Fixed remaining `F_LAST` flag checks to use proper bitmask comparison
- Fixed `MarketIfTouchedOrder` (MIT) filling at bar extremes instead of trigger price during backtesting (#3461, #3462), thanks @HaakonFlaaronning
- Fixed OTO child order sizing with rapid parent fills (#3435), thanks for reporting @dxwil
- Fixed `ExecAlgorithm` spawn quantity accounting (will now restore quantity from denied/rejected spawned orders)
- Fixed `GreeksCalculator` to use index price for index instruments (#3541), thanks @shzhng
- Fixed `GreeksCalculator` min->max DTE clamping (#3582), thanks @pandashark
- Fixed `itm_prob` calculation to use N(d2) instead of normalized delta (#3554), thanks @shzhng
- Fixed reconciliation `venue_order_id` indexing and validation
- Fixed analyzer epoch timestamp from empty shell positions
- Fixed backtest clock monotonicity with time alerts (#3384), thanks @draphi
- Fixed order updated panic during reconciliation (#3380), thanks for reporting @santivazq
- Fixed missing currency registration when adding instruments to cache (#3400), thanks @filipmacek
- Fixed trailing stops default price type (#3379), thanks @KaulSe
- Fixed typo in `OrderBook.simulate_fills` error message (#3405), thanks @Johnkhk
- Fixed registering msgbus with OptionExerciseModule (#3383), thanks @davidsblom
- Fixed directory URI handling in ParquetDataCatalog for S3 and cloud storage (#3378), thanks @KaulSe
- Fixed instrument cache race condition during `LiveNode` (Rust) startup (#3385), thanks @filipmacek
- Fixed quickstart MACD strategy logic (#3377), thanks for reporting @SisyphusCoin
- Fixed value bar aggregators emitting zero-volume bars (#3608), thanks for reporting @ggianfran
- Fixed reconciliation race condition where inferred fills were generated before real fills arrived, causing double-counting and overfill errors
- Fixed reconciliation timing (for v2 Rust) - process instruments before reconciliation (#3415), thanks @filipmacek
- Fixed `request_order_book_snapshot` and add Bybit support (#3416), thanks @dxwil
- Fixed Arrow serialization encoding for custom Nautilus types (#3515), thanks @dennisnissle
- Fixed cache loading when flush_on_start set to True (#3551), thanks @HaakonFlaaronning
- Fixed Redis cache buffer flushing during idle periods (#3426), thanks for reporting @santivazq
- Fixed Redis cache flush no-op and harden close lifecycle
- Fixed Betfair dropped fills from premature cache update
- Fixed Betfair duplicate cancel event race condition(s)
- Fixed Betfair stream batch handling and modify/cancel edge cases
- Fixed Betfair reconciliation with stale API fill data
- Fixed Binance Spot WebSocket subscription acknowledgment parsing (#3382), thanks @Johnkhk
- Fixed Binance Futures instrument parsing for margin requirements (#3420), thanks @linimin
- Fixed Binance algo order quantity `AttributeError` on `_mem` access
- Fixed Binance `cancel_all_orders` to route futures algo orders through correct cancel endpoint
- Fixed Binance Spot `OrderStatusReport.avg_px` always None (#3499), thanks for reporting @mrbaron3
- Fixed Binance Spot `client_order_id` replaced with UUID (#3500), thanks for reporting @mrbaron3
- Fixed Bybit demo trading by using HTTP REST API for order operations (Bybit demo does not support WebSocket Trade API)
- Fixed Bybit HOUR bars not triggering on_bar (#3474), thanks for reporting @88z
- Fixed Bybit historical requests to use ts_event as ts_init (#3502), thanks @dxwil
- Fixed Databento `databento_data` to fetch definitions for full date range (#3414), thanks @Johnkhk
- Fixed Databento zero-length interval at dataset boundary (#3429), thanks @shzhng
- Fixed Databento empty underlying for index-based derivatives (#3480), thanks for reporting @davidsblom
- Fixed Deribit auth token refresh race condition (#3402), thanks @filipmacek
- Fixed Deribit race condition between response and subscription (#3436), thanks @filipmacek
- Fixed Deribit grouped book channel parsing (#3473), thanks @filipmacek
- Fixed Deribit trades parsing for combo_trade_id field (#3520), thanks @davidsblom
- Fixed Interactive Brokers `fetch_all_open_orders` in client cache key preventing connection sharing (#3441), thanks @shzhng
- Fixed Interactive Brokers synthetic position order reconciliation causing filled_qty mismatch errors during periodic consistency checks (#3443), thanks @shzhng
- Fixed Interactive Brokers reconciliation error when account has no positions (#3459), thanks @shzhng
- Fixed Interactive Brokers venue determination when primaryExchange is empty (#3452), thanks @shzhng
- Fixed Interactive Brokers option symbol parsing to preserve OCC format with space padding (#3452), thanks @shzhng
- Fixed Interactive Brokers minor bugs with options (#3452), thanks @shzhng
- Fixed Interactive Brokers partial fill state transition errors where `openOrder` callbacks after fills caused invalid `PARTIALLY_FILLED` -> `ACCEPTED` transitions, thanks @shzhng
- Fixed Interactive Brokers OrderStatusReport filled_qty always being 0 for open orders causing reconciliation errors, thanks @shzhng
- Fixed Interactive Brokers external order ID collision where orders placed via TWS/other clients (orderId=0) could cause fills to be attributed to wrong orders (#3465), thanks @shzhng
- Fixed Interactive Brokers position reconciliation double-counting partial fills from open orders (#3476), thanks @shzhng
- Fixed Interactive Brokers future chain building for index instruments (#3483), thanks @davidsblom
- Fixed Interactive Brokers options missing `^` prefix on index underlying symbols with simplified symbology (#3540), thanks @shzhng
- Fixed Interactive Brokers contract for ESTX50 IND contract (#3556), thanks @davidsblom
- Fixed Interactive Brokers parsing options for Stoxx50 (#3562), thanks @davidsblom
- Fixed Interactive Brokers contract details for FESX futures (#3575), thanks @davidsblom
- Fixed Interactive Brokers `ibapi` 10.43 protobuf compatibility: `IBContract.strike` default and `ContractDetails.underConId` field typo (#3599), thanks @shzhng
- Fixed Interactive Brokers `track_option_exercise_from_position_update` not generating FLAT reports for expired options (zero-quantity position updates were silently skipped), thanks @shzhng
- Fixed Interactive Brokers bar unsubscribe (#3588), thanks for reporting @pandashark
- Fixed Kraken spot instrument fee/margin parsing where parameters were incorrectly swapped
- Fixed Kraken spot XBT to BTC symbol normalization (#3509), thanks for reporting @chester0
- Fixed OKX HTTP error messages missing rejection reason details (#3580), thanks @griffith-h
- Fixed Polymarket cancel-rejection loop for done orders
- Fixed Polymarket order state race condition where `PLACEMENT` events could arrive late
- Fixed Polymarket duplicate WebSocket subscriptions (#3403), thanks for reporting @santivazq
- Fixed Polymarket duplicate trade_id for multi-order fills (#3450), thanks for reporting @santivazq
- Fixed Polymarket `load_all_async` ignoring time-based filters (#3475), thanks @Coyote-Den
- Fixed Tardis deltas snapshot boundaries with CLEAR (#3530), thanks @Arandott

### Internal Improvements
- Added `Commodity`, `IndexInstrument`, and `Cfd` instruments in Rust
- Added support for setting cache database adapter in cache and `LiveNode` (#3401), thanks @filipmacek
- Added `ts_init` normalization option to `convert_stream_to_data` (#3433), thanks @faysou
- Added Params type and catalog instrument persistence in Rust (#3539), thanks @faysou
- Added metadata validation for parquet file consolidation to improve handling of mixed precision instruments
- Added Binance `listenKeyExpired` event handling (#3387), thanks @Johnkhk
- Added Deribit data client (#3368), thanks @filipmacek
- Added Deribit order submission (#3408), thanks @filipmacek
- Added Deribit live reconciliation support (#3421), thanks @filipmacek
- Added Deribit rate limiting for HTTP and WebSocket clients (#3424), thanks @filipmacek
- Added Deribit side-specific order cancellation (#3442), thanks @filipmacek
- Added Deribit real-time portfolio WS subscription (#3444), thanks @filipmacek
- Added Deribit integration documentation (#3508), thanks @filipmacek
- Added OKX `instIdCode` support for WebSocket order operations (#3536), thanks @Add1ct1ve
- Added Polymarket data loader rate limiting
- Migrated Nautilus internal logging to `log` crate (external `tracing` available via `use_tracing` config)
- Renamed Deribit instrument kind enum to product type (#3512), thanks @filipmacek
- Refactored execution clients to use `OrderEventEmitter` in Rust (#3469), thanks @filipmacek
- Refactored computation of greeks (#3393), thanks @faysou
- Refactored `instrument_greeks` (#3587), thanks @faysou
- Refactored `TearsheetConfig.charts` to chart objects (removed `chart_args`) (#3398), thanks @KaulSe
- Refactored Betfair order matching to use `rfo` as primary key
- Refactored Deribit WS client to use standard Nautilus method names (#3418), thanks @filipmacek
- Refactored dYdX v4 execution client in Rust (#3477), thanks @filipmacek
- Refactored dYdX v4 adapter (#3521), thanks @filipmacek
- Refactored dYdX v4 data client (#3547), thanks @filipmacek
- Refactored dYdX v4 execution client (#3557), thanks @filipmacek
- Refactored Kraken spot quotes to use dedicated Ticker channel
- Refactored Polymarket WebSocket to multi-client pool pattern
- Improved `cancel_all_orders` to include inflight orders
- Improved pnl FX conversions in portfolio (#3335), thanks @faysou
- Improved live timers to use `BTreeMap` for storage (#3392), thanks @faysou
- Improved checks before writing data in catalog._write_chunk (#3411), thanks @faysou
- Improved `ts_init` monotonicity enforcement in `convert_stream_to_data` (#3600), thanks @faysou
- Improved `OptionExerciseModule` logging and fix cache reference (#3388), thanks @davidsblom
- Improved execution reports builder pattern in Rust (#3417), thanks @filipmacek
- Improved `GridMarketMaker` strategy and dYdX cancel handling (#3601), thanks @filipmacek
- Improved visualization to use fill report for create_bars_with_fills (#3466), thanks @faysou
- Improved Architect AX WebSocket data and order handling (#3577), thanks @andrew-cho-architect
- Improved Betfair adapter rate limiting and fill deduplication
- Improved Deribit with high-performance `Decimal` deserialization (#3510), thanks @filipmacek
- Improved precision-mode validation for Arrow data (#3511), thanks for reporting @2-5
- Improved dYdX v4 data client subscription handling (#3537), thanks @filipmacek
- Improved dYdX v4 rate limiting and cancel strategy (#3606), thanks @filipmacek
- Improved dYdX v4 docs and grid market making tutorial (#3607)
- Refined closing of streaming writer (#3394), thanks @faysou
- Refined handling of `skip_first_non_full_bar` in `TimeBarAggregator` (#3395), thanks @faysou
- Refined greeks safeguards and docs (#3407), thanks @faysou
- Refined processing of gaps in aggregated historical bars (#3412), thanks @faysou
- Refined exercise and settlement of expiring instruments (#3531), thanks @faysou
- Refined `OptionExerciseModule` (#3423), thanks @faysou
- Refined instrument `is_spread()` method (#3434), thanks @faysou
- Refined `OrderBookDeltas.batch` (#3437), thanks @faysou
- Refined conversion of feather files to parquet (#3590), thanks @faysou
- Refined Interactive Brokers adapter (#3195), thanks @faysou
- Refined Interactive Brokers query of option chains (#3481), thanks @faysou
- Refined Interactive Brokers parsing of alternative option symbol format (#3564), thanks @faysou
- Optimized `Price::from_decimal` with integer arithmetic
- Optimized `Quantity::from_decimal` with integer arithmetic
- Optimized `Money::from_decimal` with integer arithmetic
- Optimized message bus publish with thread-local `SmallVec` buffers in Rust
- Optimized message bus pattern matching with greedy algorithm
- Upgraded Interactive Brokers adapter to `ibapi` 10.43 (#3427, #3595), thanks @faysou
- Upgraded Rust (MSRV) to 1.93.1
- Upgraded Cap'n Proto to v1.3.0
- Upgraded Cython to v3.2.4
- Upgraded `databento` crate to v0.41.0
- Upgraded `datafusion` crate to v52.1.0
- Upgraded `pyo3` crate to v0.28.2
- Upgraded `pyo3-async-runtimes` crate to v0.28.0
- Upgraded `redis` crate to v1.0.4
- Upgraded `tokio` crate to v1.49.0

### Documentation Updates
- Added related guides sections to concepts
- Added developer guide for test dataset standards
- Added AX Exchange adapter integration guides
- Added Deribit adapter integration guides
- Split dYdX v3/v4 adapter integration guides

### Deprecations
- Deprecated Betfair legacy `customer_order_ref` truncation (first 32 characters); the adapter now uses last 32 characters for better entropy. Legacy truncation support during startup reconciliation will be removed in a future version.
- Deprecated Binance `key_type` config field; key type is now auto-detected (only needed if explicitly using RSA keys)
- Deprecated Binance `testnet` config field; use `environment=BinanceEnvironment.TESTNET` instead
- Deprecated Binance `BINANCE_ED25519_*` and `BINANCE_*_ED25519_*` environment variables; migrate to the standard `BINANCE_API_KEY`/`BINANCE_API_SECRET` variables

---

# NautilusTrader 1.222.0 Beta

Released on 1st January 2026 (UTC).

This release adds support for Python 3.14 with the following limitations:
- dYdX adapter extras (`[dydx]`) unavailable due to upstream `coincurve` compatibility (available on Python 3.12-3.13)
- Interactive Brokers adapter extras (`[ib]`) unavailable due to upstream `nautilus-ibapi` compatibility (available on Python 3.12-3.13)

### Enhancements
- Added support for Python 3.14
- Added Kraken integration adapter
- Added Cap'n Proto (`capnp`) serialization for efficient zero-copy data interchange (opt-in via `capnp` feature flag in `nautilus-serialization` crate)
- Added initial backtest visualization tearsheets with plotly
- Added matching engine `liquidity_consumption` config option to track per-level consumption and prevent overfilling displayed book liquidity (default `False` to retain current behavior)
- Added matching engine trade consumption tracking (when `liquidity_consumption=True` and `trade_execution=True`) to prevent multiple orders matching the same trade tick from collectively overfilling
- Added theme support to `bars_with_fills` chart (#3329), thanks @faysou
- Added price protection support for market orders (#3065), thanks @Antifrajz
- Added `Quantity.from_decimal` constructor (#3189), thanks @faysou
- Added `Price.from_decimal` constructor
- Added `Money.from_decimal` constructor
- Added `create_bars_with_fills` to Tearsheet (#3137), thanks @faysou
- Added `proxy_url` support for HTTP clients
- Added `CAGR` portfolio statistic
- Added `CalmarRatio` portfolio statistic
- Added `MaxDrawdown` portfolio statistic
- Added `quote_quantity` parameter for `close_position(...)` and `close_all_positions(...)` strategy methods
- Added remaining bar aggregation methods: `TICK_IMBALANCE`, `TICK_RUNS`, `VOLUME_IMBALANCE`, `VOLUME_RUNS`, `VALUE_IMBALANCE`, `VALUE_RUNS` (#3217), thanks @nicolad
- Added `ParquetDataCatalog.query_first_timestamp` (#3253), thanks @MK27MK
- Added `PolymarketDataLoader` for loading historical data with docs and example
- Added Binance accurate commission rates per symbol (#3208), thanks @delusionpig
- Added Binance cross-margin info to `AccountState`
- Added `BinanceInstrumentProviderConfig` to support the `query_commission_rates` config option
- Added Bybit spot margin auto-borrow and auto-repay with `auto_repay_spot_borrows` config option
- Added Bybit spot margin manual operations (`BybitMarginAction`) for strategy-controlled borrow/repay via `query_account`
- Added Bybit HTTP request_tickers support (#3241), thanks @TaiShanQ
- Added Databento subscription acknowledgement handling (#3337), thanks @shzhng
- Added Databento historical client consolidated schema support (#3338), thanks @shzhng
- Added Interactive Brokers optional exchange param for spread contracts (#3319), thanks @faysou
- Added Polymarket Gamma API support for instrument loading (#3141), thanks @DeirhX
- Added OKX historical trades requests
- Added Tardis `book_snapshot_output` config option for tardis machine replays (default `deltas` to retain current behavior)
- Added `allow_overfills` config option to `ExecEngineConfig` (default `False`) to handle order fills exceeding order quantity with warning instead of raising
- Added `overfill_qty` field to orders for tracking fill quantities exceeding original order quantity
- Introduced `PositionAdjusted` events for tracking quantity/PnL changes outside normal order fills (base currency commissions, funding payments, manual adjustments)
- Upgraded continuous reconciliation for execution engine using position reports to detect missed fills

### Breaking Changes
- Dropped support for Python 3.11
- Removed `prob_fill_on_stop` parameter from `FillModel` and `FillModelConfig` (stop orders have no queue position to simulate as triggers are deterministic when price reaches the trigger level)
- Removed `use_ws_trade_api` config option from Bybit execution client (using WebSocket trade API only); this inadvertently broke demo trading since Bybit demo does not support WebSocket Trade API
- Renamed `parse_instrument` to `parse_polymarket_instrument` in Polymarket adapter for clarity
- Renamed `ExecTesterConfig.enable_buys` to `enable_limit_buys`
- Renamed `ExecTesterConfig.enable_sells` to `enable_limit_sells`
- Changed `ParquetDataCatalog.register_data` to now treat `files=[]` as registering no files; pass `files=None` (default) to include all files
- **Standardized data catalog directory naming**: Order book data directory names now use plural forms to align with the Rust catalog and Tardis Machine conventions; this ensures data written by the Python `StreamingFeatherWriter` can be read by the Rust catalog
  - `order_book_delta/` → `order_book_deltas/`
  - `order_book_depth10/` → `order_book_depths/`

  **Migration**: Rename existing data directories to use plural forms:
  ```bash
  # If you have existing order book data, rename the directories:
  mv <your_data_path>/order_book_delta <your_data_path>/order_book_deltas
  mv <your_data_path>/order_book_depth10 <your_data_path>/order_book_depths
  ```

### Security
- Added `osv-scanner` for Python dependency vulnerability scanning in pre-commit
- Added `cargo-vet` for Rust supply chain security auditing
- Hardened unsafe code with runtime checks and `#![deny(unsafe_op_in_unsafe_fn)]` lint
- Hardened datetime conversions with overflow protection
- Hardened CI workflows by pinning Docker images to SHA digests
- Improved actor/component registry safety with `ActorRef` guards and runtime borrow tracking
- Fixed code scanning security alerts

### Fixes
- Fixed `uint64_t` truncation bug in `determine_trade_fill_qty` for trade execution with `high-precision` mode
- Fixed stop market order fill price in `L1_MBP` mode
- Fixed cache dropped same-timestamp market data on insert
- Fixed race condition in InstrumentProvider causing duplicate instrument initialization in shared providers
- Fixed portfolio statistics various bugs and edge cases
- Fixed SyntheticInstrument formula error during parsing with hyphened InstrumentId (#3257), thanks @Javdu10
- Fixed balance recalculation to use raw fixed-point (#3356), thanks @kirill-gr1
- Fixed matching engine GTD order expiry key mismatch (#3272), thanks for reporting @linimin
- Fixed matching engine order modification for partial fills
- Fixed matching engine L2/L3 partial fill quantity calculation on subsequent book updates
- Fixed NETTING position flip snapshots and cache index cleanup (#3081), thanks @SarunasSS
- Fixed incorrect handling of data responses in msgbus (#3310), thanks @filipmacek
- Fixed data engine to use separate aggregators for historical data (#3326), thanks @faysou
- Fixed bar execution generating fractional fill quantities (#3352), thanks @Johnkhk
- Fixed `BacktestResult.total_positions` to match tearsheet count (#3148), thanks for reporting @2-5
- Fixed risk engine negative price handling for spread instruments (#3136), thanks for reporting @q351941406
- Fixed risk engine trailing stop order risk validations (#3160), thanks for reporting @GianC0
- Fixed risk engine balance checks for cash borrowing
- Fixed risk engine balance checks for position-reducing SELL orders (#3256), thanks for reporting @GianC0
- Fixed spawned order client_id caching in `ExecAlgorithm` (#3122), thanks for reporting @kirill-gr1
- Fixed parse_dates parameter in CSV loaders (#3132), thanks @maomao9-0
- Fixed `GreeksCalculator` handling of missing price data (#3116), thanks for reporting @q351941406
- Fixed `StreamingFeatherWriter` `_setup_streaming` with `replace_existing` config (#3234), thanks @cauta
- Fixed conversion of streamed instruments to catalog (#3235), thanks @faysou
- Fixed active liquidity calculation Pool profiler simulation (#3165), thanks @filipmacek
- Fixed duplicate `on_instrument` callback in request flow for Python adapters (#3323), thanks @filipmacek
- Fixed Redis index key parsing with `use_instance_id`
- Fixed Betfair datetime encoding error in order status reports
- Fixed Betfair login race condition during concurrent connections
- Fixed Betfair parsing errors for undocumented codes
- Fixed Betfair duplicate fills on startup/reconnect
- Fixed Binance instrument info dict JSON serialization (#3128), thanks for reporting @woung717
- Fixed Binance ADL orders with TRADE execution type
- Fixed Binance Futures Algo Order API for conditional orders (#3287), thanks for reporting @KaizynX
- Fixed Bybit historical bars requests partial (unclosed) bar filtering
- Fixed Bybit WebSocket bars to respect `timestamp_on_close` config
- Fixed `BybitHttpClient` type stub pyi signatures (#3238), thanks @sunlei
- Fixed Databento historical client to support consolidated schemas (`cmbp-1`, `cbbo-1s`, `cbbo-1m`) in quote requests
- Fixed Databento MBO data decoding when `PRICE_UNDEF` appears with non-zero precision
- Fixed Databento Arrow serialization for `PRICE_UNDEF` (#3183), thanks for reporting @marloncalvo
- Fixed Databento quote decoding with undefined bid/ask prices
- Fixed Interactive Brokers quote tick subscriptions to use tick-by-tick data (#3135), thanks for reporting @genliusrocks
- Fixed Interactive Brokers serialization of `IBContractDetails` (#3181), thanks @faysou
- Fixed Interactive Brokers parsing of invalid prices (#3246), thanks @faysou
- Fixed OKX pre-open instrument parsing and standardize enum usage (#3134), thanks for reporting @3wtz
- Fixed OKX `request_bars` pagination halting prematurely in Range mode (#3145), thanks for reporting @3wtz
- Fixed OKX `request_bars` pagination using correct backwards API semantics (#3145), thanks for reporting @3wtz
- Fixed OKX FOK/IOC order type preservation across parsers (#3182), thanks @CuBeof
- Fixed OKX fee rate sign convention for backtesting (#3260), thanks @GhostLee
- Fixed Polymarket maker fill order side inversion (#3126), thanks for reporting @santivazq
- Fixed Polymarket instrument provider market filtering (#3133), thanks @MisterMM23
- Fixed Polymarket websocket client cancellation on concurrent subscriptions (#3169), thanks @DeirhX
- Fixed Polymarket maker fills parsing for cross-asset matching and multiple concurrent fills (#3172), thanks @petioptrv
- Fixed Polymarket account balance update timing issue (#3161), thanks for reporting @santivazq
- Fixed Polymarket handling of overfilled FOK orders using `allow_overfills` execution engine config option (#3221), thanks for reporting @Javdu10
- Fixed Polymarket `match_time` timestamp parsing (#3273), thanks for reporting @santivazq
- Fixed Polymarket timestamp conversions (#3291, #3292), thanks for reporting @santivazq
- Fixed Polymarket fill reports for cross-asset matches (#3345), thanks for reporting @santivazq
- Fixed Polymarket order side for cross-asset matches (#3357), thanks for reporting @santivazq
- Fixed Tardis book snapshot to deltas CLEAR prepending
- Fixed Tardis CSV parsing for mid-day snapshots

### Internal Improvements
- Added BitMEX submit broadcaster
- Added Bybit start/end time filtering for order status reports (#3209), thanks @sunlei
- Added BybitRawHttpClient Python bindings (#3252), thanks @sunlei
- Added Databento subscription acknowledgement handling and logging
- Added non-mutating swap quote simulation for Pool tickmap profiling (#3123), thanks @filipmacek
- Added ERC20 token balance tracking to BlockchainExecutionClient (#3224), thanks @filipmacek
- Added DeFi pool discovery service with full Uniswap(V2/V3/V4) support (#3255), thanks @filipmacek
- Added Deribit HTTP client with instrument support (#3288), thanks @filipmacek
- Added Deribit account balance and credential management (#3295), thanks @filipmacek
- Added Deribit WebSocket client with market data support (#3297), thanks @filipmacek
- Added Deribit WebSocket auth and raw data stream support (#3304), thanks @filipmacek
- Added Deribit data client in Rust (#3311), thanks @filipmacek
- Added Deribit data client Python bindings (#3315), thanks @filipmacek
- Added Deribit data client WebSocket handling and request methods (#3340), thanks @filipmacek
- Added Deribit execution client scaffolding (#3350), thanks @filipmacek
- Added dYdX v4 crate (#3138), thanks @nicolad
- Added dYdX v4 WebSocket in Rust (#3158), thanks @nicolad
- Added dYdX v4 DataClient in Rust (#3162), thanks @nicolad
- Added dYdX v4 ExecutionClient in Rust (#3163), thanks @nicolad
- Added dYdX v4 execution reconciliation in Rust (#3171), thanks @nicolad
- Added dYdX v4 gRPC order execution (#3222), thanks @nicolad
- Added dYdX v4 order execution via gRPC with Python bindings (#3245), thanks @nicolad
- Added dYdX v4 conditional orders (#3259), thanks @nicolad
- Added dYdX v4 Python adapter layer (#3275), thanks @nicolad
- Added dYdX v4 batch cancel and expose missing Python bindings (#3282), thanks @nicolad
- Added dYdX v4 HTTP data, execution, and WebSocket tests (#3290), thanks @nicolad
- Added Kraken Futures demo support (#3262), thanks @nicolad
- Added check before creation of bars in IB adapter (#3348), thanks @PJPRoche and @faysou
- Added check for empty data in _handle_table_nautilus (#3248), thanks @faysou
- Integrated trade analytics across DeFi pools swaps and simulated quotes (#3174), thanks @filipmacek
- Implemented size for impact bps `PoolProfiler` simulation (#3186), thanks @filipmacek
- Implemented dual-parser architecture for DEX event parsing (#3228), thanks @filipmacek
- Implemented Bybit chunking support for batch cancel orders (#3244), thanks @sunlei
- Scaffolded blockchain execution client with native balance fetch (#3214), thanks @filipmacek
- Ported Bybit integration adapter to Rust
- Unified tokio runtime selection in Rust adapters (#3321), thanks @filipmacek
- Converted `LatencyModel` to trait with `StaticLatencyModel` impl (#3369), thanks @marcus-sa
- Refactored network crate to modularize `http`, `socket`, and `websocket`
- Refactored reading of feather files in catalog (#3114), thanks @faysou
- Refactored processing of historical data (#3038), thanks @faysou
- Refactored execution engine reconciliation (#3185), thanks @faysou
- Refactored risk engine initialization with shallow clone for portfolio (#3360), thanks @marcus-sa
- Refactored `SpreadQuoteAggregator` (#3312), thanks @faysou
- Refactored Polymarket instrument provider to use async HttpClient
- Refactored Interactive Brokers `HistoricInteractiveBrokersClient` (#3261), thanks @faysou
- Refactored IB Historical client (#3276), thanks @faysou
- Improved trade execution matching with transient bid/ask override for `trade_execution=True` mode, ensuring limit orders fill correctly when trades occur at the limit price
- Improved Stochastics indicator with additional parameters (#3296), thanks @mahmutf
- Improved `None` handling in equality and comparison methods
- Improved `Actor.request_bars` to enforce standard bar types (#3216), thanks @faysou
- Improved JSON-RPC non-standard rate limit error handling (#3227), thanks @filipmacek
- Improved Betfair execution error handling and edge cases
- Improved Betfair order rejection and duplicate fills handling
- Improved Binance data client with optional authentication
- Improved Bybit spot borrow repayments (#3223), thanks @vcraciun
- Improved Databento live connection stability and reconnects
- Improved Databento decoder sentinel value handling (#3361), thanks for reporting @davidsblom
- Improved dYdX v3 resilience and reliability (#3225), thanks @SarunasSS
- Improved dYdX v4 adapter test coverage (#3212), thanks @nicolad
- Improved dYdX v4 network, bars, and batch cancel (#3231), thanks @nicolad
- Improved dYdX v4 gRPC execution with edge cases and batch cancel (#3239), thanks @nicolad
- Improved dYdX v4 data/exec testers and fix GTT (#3254), thanks @nicolad
- Improved dYdX v4 WebSocket subscription state management (#3286), thanks @nicolad
- Improved dYdX v4 enums for type safety and improve WS tests (#3294), thanks @nicolad
- Improved dYdX v4 model type safety with enums (#3299), thanks @nicolad
- Improved dYdX v4 parse block height WebSocket feed and gate short-term order submission (#3320), thanks @nicolad
- Improved Polymarket position querying using Gamma API (#3142), thanks @DeirhX
- Improved Tardis adapter robustness and error handling
- Standardized dYdX WebSocket architecture (#3173), thanks @nicolad
- Standardized dYdX client integration tests (#3193), thanks @nicolad
- Standardized dYdX per adapter guide conventions (#3267), thanks @nicolad
- Changed Interactive Brokers default quote tick subscription to batch quotes (#3196), thanks @faysou
- Changed spread quote aggregation to opt-in (#3355), thanks @faysou
- Removed redundant debug code for reconcile execution (#3344), thanks @TaiShanQ
- Refined timer name validation to accept non-ASCII characters (common for foreign currencies) (#3154), thanks for reporting @woung717
- Refined spread support (#3284), thanks @faysou
- Refined support for monthly and yearly bars (#3166), thanks @faysou
- Refined bar aggregators in Rust (#3170), thanks @faysou
- Refined adding files to catalog session (#3215), thanks @faysou
- Refined loading of files in catalog (#3313), thanks @faysou
- Refined catalog file filter methods (#3318), thanks @faysou
- Refined `HistoricInteractiveBrokersClient` (#3187), thanks @faysou
- Refined `BacktestDataIterator` docstrings (#3264), thanks @faysou
- Refined `BacktestDataConfig.query` (#3266), thanks @faysou
- Refined Databento utils (#3268), thanks @faysou
- Refined Interactive Brokers historical data request methods (#3279), thanks @faysou
- Refined requests and aggregators (#3328), thanks @faysou
- Refined parsing of IB expiries (#3332), thanks @faysou
- Refined subscription to spread quotes (#3349), thanks @faysou
- Refined data query and subscription (#3353), thanks @faysou
- Refined response to join_request (#3366), thanks @faysou
- Refined adding instrument to cache after modifying it (#3372), thanks @faysou
- Optimized unnecessary string allocations and `Ustr` usage
- Optimized build to prefer sccache when available (#3243), thanks @sunlei
- Optimized execution reconciliation to avoid quadratic complexity (#3140), thanks @DeirhX
- Optimized network clients by enabling `TCP_NODELAY` (#3156), thanks @sunlei
- Optimized build by disabling Cargo incremental compilation when using sccache (#3157), thanks @sunlei
- Optimized BitMEX submit and cancel broadcasters by removing unnecessary lock on internal transport clients
- Optimized full math division for DeFi calculations (#3179), thanks @filipmacek
- Optimized parquet data filtering and streaming initialization performance (#3298), thanks @ReCodeLife
- Repaired OKX spot margin position reports for borrowing, thanks @sunlei
- Repaired Bybit docs links in comment (#3125), thanks @sunlei
- Repaired Bybit HTTP order place (#3127), thanks @sunlei
- Repaired Bybit `AccountPosition` message parsing (#3147), thanks @sunlei
- Repaired Bybit conditional order trigger semantics and type
- Repaired Bybit instruments pagination handling (#3210), thanks @sunlei
- Repaired Bybit batch place orders (#3211), thanks @sunlei
- Repaired Bybit `get_account_details` (#3219), thanks @sunlei
- Repaired Bybit `set_position_mode` (#3220), thanks @sunlei
- Upgraded implied-vol crate (#3115), thanks @faysou
- Upgraded Rust (MSRV) to 1.92.0
- Upgraded Cython to v3.2.3
- Upgraded `databento` crate to v0.37.0
- Upgraded `datafusion` crate to v51.0.0
- Upgraded `msgspec` to v0.20.0
- Upgraded `pyo3` crate to v0.27.2
- Upgraded `pyo3-async-runtimes` crate to v0.27.0
- Upgraded `redis` crate to v1.0.2

### Documentation Updates
- Added Polymarket historical data loading docs
- Added visualization docs for `bars_with_fills` tearsheet feature
- Added order state flow diagram with lifecycle documentation
- Added fee rate sign convention in instruments concept guide
- Added fill price determination to backtesting concept guide
- Improved concept docs with Mermaid diagrams replacing ASCII diagrams
- Improved execution concept guide with overfills explanation
- Improved backtesting concept guide to clarify bar execution behavior
- Improved documentation for uv-installed Python environments, thanks to @faysou for investigating and reporting
- Improved notebook path handling and fix quickstart data loading, thanks for reporting @semihtekten

### Deprecations
None

---

# NautilusTrader 1.221.0 Beta

Released on 26th October 2025 (UTC).

This will be the final release with support for Python 3.11.

### Enhancements
- Added support for `OrderBookDepth10` requests (#2955), thanks @faysou
- Added support for quotes from book depths (#2977), thanks @faysou
- Added support for quotes from order book deltas updates (#3106), thanks @faysou
- Added execution engine rate limiting for single-order reconciliation queries
- Added `subscribe_order_fills(...)` and `unsubscribe_order_fills(...)` for `Actor` allowing to subscribe to all fills for an instrument ID
- Added `on_order_filled(...)` for `Actor`
- Added Renko bar aggregator (#2941), thanks @faysou
- Added `time_range_generator` for on-the-fly data data subscriptions (#2952), thanks @faysou
- Added `__repr__` to `NewsEvent` (#2958), thanks @MK27MK
- Added `convert_quote_qty_to_base` config option to `ExecEngineConfig` (default `True` to retain current behavior) allows adapters to keep quote-denominated sizes when needed
- Added contingent order fields `parent_order_id` and `linked_order_ids` for `OrderStatusReport` and reconciliation
- Added `fs_rust_storage_options` to Python catalog (#3008), thanks @faysou and @Johnkhk
- Added matching engine fallback to default order book for custom fill models (#3039), thanks @Hamish-Leahy
- Added filesystem parameter to parquet in the consolidate functions (#3097), thanks @huracosunah
- Added azure support for az protocol (#3102), thanks @huracosunah
- Added Binance BBO `price_match` parameter support for order submission
- Added BitMEX conditional orders support
- Added BitMEX batch cancel support
- Added BitMEX contingent orders support (OCO, OTO, brackets)
- Added BitMEX historical data requests (trades and bars)
- Added BitMEX configurable `recv_window_ms` for signed HTTP request expiration
- Added Bybit SPOT position reports with opt-in `use_spot_position_reports` config option for `BybitExecClientConfig`
- Added Bybit `ignore_uncached_instrument_executions` config option for `BybitExecClientConfig` (default `False` to retain current behavior)
- Added Databento CME sandbox example
- Added Interactive Brokers cache config support for historical provider (#2942), thanks @ms32035
- Added Interactive Brokers support for fetching orders from all clients (#2948), thanks @dinana
- Added Interactive Brokers order conditions (#2988), thanks @faysou
- Added Interactive Brokers `generate_fill_reports` implementation (#2989), thanks @faysou
- Added OKX conditional trigger orders support
- Added OKX trade mode per order via `params` using `td_mode` key
- Added OKX margin configuration and spot margin support
- Added OKX demo account support
- Added OKX batch cancel support
- Added Polymarket native market orders support

### Breaking Changes
- Removed `nautilus_trader.analysis.statistics` subpackage - all statistics are now implemented in Rust and must be imported from `nautilus_trader.analysis` (e.g., `from nautilus_trader.analysis import WinRate`)
- Removed partial bar functionality from bar aggregators and subscription APIs (#3020), thanks @faysou
- Renamed `nautilus-cli` crate feature flag from `hypersync` to `defi` (gates blockchain/DeFi commands)
- Polymarket execution client no longer accepts market BUY orders unless `quote_quantity=True`

### Security
- Fixed non-executable stack for Cython extensions to support hardened Linux systems
- Fixed divide-by-zero and overflow bugs in model crate that could cause crashes
- Fixed core arithmetic operations to reject NaN/Infinity values and improve overflow handling

### Fixes
- Fixed reduce-only order panic when quantity exceeds position
- Fixed position purge logic to prevent purging re-opened position
- Fixed `Position.purge_events_for_order` to properly rebuild state from remaining order fills
- Fixed cache index cleanup bugs in purge_order operations
- Fixed order average price calculation that was double-counting current fill in weighted average
- Fixed own order book cleanup for terminal orders and inflight handling
- Fixed order book depth snapshot processing to avoid padding levels and metadata tracking for L1 top-of-book ticks
- Fixed crypto instruments PyO3 -> Cython conversion for `lot_size` where it was not being passed through
- Fixed `serialization` crate bugs and improve error handling
- Fixed PyO3 interpreter lifecycle for async shutdown preventing edge case `"interpreter not initialized"` panics during shutdown
- Fixed `RiskEngine` reduce-only cash exits (#2986), thanks for reporting @dennisnissle
- Fixed `RiskEngine` quote quantity validation
- Fixed `BacktestEngine` to retain instruments on reset (#3096), thanks for reporting @woung717
- Fixed overflow in `NautilusKernel` build time calculation due to negative duration (#2998), thanks for reporting @HaakonFlaaronning
- Fixed handling of asyncio.CancelledError in execution reconciliation (#3073), thanks @dinana
- Fixed edge case where rejected orders can remain in own order book
- Fixed Currency registration to synchronize between Cython and PyO3 runtimes via new `register_currency()` helper
- Fixed Databento CMBP-1/CBBO/TBBO symbology resolution
- Fixed `on_load` called before strategy added bug (#2953), thanks @lisiyuan656
- Fixed filesystem usage in catalog for `isfile` and `isdir` (#2954), thanks @limx0
- Fixed `ParquetDataCatalog.from_uri` to support Windows paths (#3283), thanks @nikitium
- Fixed `SandboxExecutionClient` instrument data handling
- Fixed `AccountState` Arrow serialization (#3005), thanks for reporting @nikzasel
- Fixed `CryptoOption` Arrow schema `option_kind` field to accept string values
- Fixed `FuturesSpread` Arrow schema missing max/min quantity and price fields
- Fixed `OptionSpread` Arrow schema missing max/min quantity and price fields
- Fixed `Commodity` Arrow schema to match from_dict requirements
- Fixed safe encoded symbols (#2964), thanks @ms32035
- Fixed msgspec encoding for type objects with qualified names
- Fixed nautilus CLI macOS compatibility with regex unicode-perl feature (#2969), thanks @learnerLj
- Fixed fuzzy candlesticks indicator bugs (#3021), thanks @benhaben
- Fixed return type annotation for `ArrowSerializer.deserialize` (#3076), thanks @MK27MK
- Fixed initializing of sqrt price setting flow when `Pool` profiling (#3100), thanks @filipmacek
- Fixed Redis multi-stream consumer skipping messages (#3094), thanks for reporting @kirill-gr1
- Fixed Binance duplicate `OrderSubmitted` event generation for order lists (#2994), thanks @sunlei
- Fixed Binance websocket fill message parsing for Binance US with extra fields (#3006), thanks for reporting @bmlquant
- Fixed Binance order status parsing for external orders (#3006), thanks for reporting @bmlquant
- Fixed Binance execution handling for self-trade prevention and liquidations (#3006), thanks for reporting @bmlquant
- Fixed Binance trailing stop to use server-side activation price (#3056), thanks for reporting @hope2see
- Fixed Binance Futures reconciliation duplicated position bug (#3067), thanks @lisiyuan656
- Fixed Binance `price_match` order price synchronization (#3074)
- Fixed Binance Futures position risk query to use v3 API returning only symbols with positions or open orders (#3062), thanks for reporting @woung717
- Fixed Binance Futures liquidation and ADL fill handling
- Fixed BitMEX testnet support
- Fixed BitMEX instrument parsing of lot size
- Fixed BitMEX order rejection handling and response parsing
- Fixed Blockchain adapter out of gas RPC error in Multicall for problematic contracts (#3086), thanks @filipmacek
- Fixed Bybit currency parsing from venue resulting in incorrectly low precision (e.g., USDT precision 4 rather than 8)
- Fixed Bybit handling of `OrderModifyRejected` events from pending updates
- Fixed Bybit account endpoint pagination handling
- Fixed Coinbase Intx API credentials handling to allow passing explicitly
- Fixed Databento MBO `Clear` actions and improve docs
- Fixed Hyperliquid L1 signing with direct MessagePack serialization (#3087), thanks @nicolad
- Fixed Interactive Brokers tick level historical data downloading (#2956), thanks @DracheShiki
- Fixed Interactive Brokers instrument provider `TypeError` when load_ids/contracts are `None`, thanks for reporting @FGU1
- Fixed Interactive Brokers modify bracket order (#2979), thanks @faysou
- Fixed Interactive Brokers historical bars resubscription failure after connection loss (#3002), thanks @Johnkhk
- Fixed Interactive Brokers flat position reconciliation and instrument loading (#3023), thanks @idobz
- Fixed Interactive Brokers bars response handling by removing partial bar (#3040), thanks @sunlei
- Fixed Interactive Brokers account summary handling (#3052), thanks @shinhwasbiz02
- Fixed Interactive Brokers account balance calculation (#3064), thanks @sunlei
- Fixed OKX spot margin quote quantity order handling
- Fixed OKX API credentials handling to allow passing explicitly
- Fixed OKX fee calculations to account for negative fees
- Fixed OKX parsing for `tick_sz` across instrument types
- Fixed OKX parsing for instruments `multiplier` field
- Fixed OKX WebSocket heartbeat and standardize logging
- Fixed Polymarket handling of one-sided quotes (#2950), thanks for reporting @thefabus
- Fixed Polymarket websocket message handling (#2963, #2968), thanks @thefabus
- Fixed Polymarket tick size change handling for quotes (#2980), thanks for reporting @santivazq
- Fixed Polymarket market order submission to use native CLOB market orders (#2984), thanks for reporting @njkds
- Fixed Polymarket maker fill order side inversion (#3077), thanks for reporting @DarioHett
- Fixed Polymarket `neg_risk` order parameter handling
- Fixed Tardis instruments `lot_size` mapping
- Fixed Tardis adapter error handling and connection robustness
- Fixed Tardis replay to use catalog-compatible filenames

### Internal Improvements
- Added ARM64 support to Docker builds
- Added BitMEX adapter integration tests
- Added OKX adapter integration tests
- Added turmoil network simulation testing to network crate
- Added liquidity utilization rate to AMM pool profiler (#3107), thanks @filipmacek
- Added `filter_sec_types` config to skip unsupported IB instrument types (#3108), thanks @sunlei
- Ported `PortfolioAnalyzer` and all portfolio statistics to Rust
- Introduced AMM Pool profiler with tickmaps and Uniswapv3 support (#3000, #3010, #3019, #3036), thanks @filipmacek
- Introduced snapshot, analytics, and PSQL schema for PoolProfiler (#3048), thanks @filipmacek
- Implemented consistency checking for AMM pool profiler with RPC state (#3030), thanks @filipmacek
- Implemented `PoolFlash` event in blockchain adapter (#3055, #3058), thanks @filipmacek
- Implemented Blockchain adapter pool profiler snapshot integration (#3090), thanks @filipmacek
- Implemented BitMEX robust ping/pong handling
- Implemented Hyperliquid adapter HTTP client (#2939), thanks @nicolad
- Implemented Hyperliquid adapter scaffolding and examples (#2957), thanks @nicolad
- Implemented Hyperliquid weighted rate limiter for REST API (#2960), thanks @nicolad
- Implemented Hyperliquid L2 order book with tick-based pricing (#2967), thanks @nicolad
- Implemented Hyperliquid data client and fix dependencies (#2975), thanks @nicolad
- Implemented Hyperliquid REST API models for execution (#2983), thanks @nicolad
- Implemented Hyperliquid `InstrumentProvider` / definitions parsing (#2992), thanks @nicolad
- Implemented Hyperliquid DataClient in Python (#2996), thanks @nicolad
- Implemented Hyperliquid DataClient in Rust (#2999), thanks @nicolad
- Implemented Hyperliquid ExecutionClient in Python (#3003), thanks @nicolad
- Implemented Hyperliquid ExecutionClient in Rust (#3013), thanks @nicolad
- Implemented Hyperliquid websocket tester for streaming market data (#3018), thanks @nicolad
- Implemented Hyperliquid basic market and limit orders (#3022), thanks @nicolad
- Implemented Hyperliquid conditional / advanced orders (#3035), thanks @nicolad
- Implemented Hyperliquid execution reconciliation (#3041), thanks @nicolad
- Implemented Hyperliquid execution client order submission (#3050), thanks @nicolad
- Implemented Hyperliquid LiveExecutionClientExt trait (#3075), thanks @nicolad
- Implemented Hyperliquid typed enums and optimize WebSocket lookups (#3089), thanks @nicolad
- Refactored Hyperliquid adapter to push complexity to Rust layer (#3063), thanks @nicolad
- Refactored streaming writer to support per-bar-type persistence (#3078), thanks @faysou
- Changed `Symbol`, `Currency`, and `InstrumentId` string validation from ASCII to UTF-8, fixing Binance compatibility with Chinese symbols
- Changed `PositionId` validation check from ASCII to UTF-8, fixing Binance compatibility with Chinese symbols (#3105), thanks @Osub
- Improved clock and timer thread safety and validations
- Improved live timer lifecycle management by canceling existing timers with the same name
- Improved `ActorExecutor` lifecycle and concurrency handling
- Improved order book error handling, state integrity, and pprint/display
- Improved order book handling of `NoOrderSide` deltas
- Improved websocket reconnection sequence protections in stream mode
- Improved socket reconnect sequence and tighten client setup and testing
- Improved socket client URL parsing
- Improved compatibility of Makefile for Windows git-bash (#3066), thanks @faysou
- Improved Blockchain adapter shutdown with cancellation token
- Improved Blockchain adapter `node_test` script (#3092), thanks @filipmacek
- Improved and optimize AMM pool profiling (#3098), thanks @filipmacek
- Improved Hyperliquid adapter patterns (#2972), thanks @nicolad
- Improved BitMEX spot instruments quantity handling by scaling to correct fractional units
- Improved BitMEX REST rate limits configuration
- Improved BitMEX instrument cache error logging
- Improved Binance, Bybit, OKX, BitMEX, and Coinbase International HTTP rate limiting to enforce documented per-endpoint quotas
- Improved Binance fill handling when instrument not cached with clearer error log
- Improved dYdX v4 websocket lifecycle and add fixture-based tests (#3285), thanks @nicolad
- Improved OKX trade mode detection and fee currency parsing
- Improved OKX client connection reliability
- Improved OKX liquidation and ADL fill handling and logging
- Improved Tardis instrument requests to filter options by default
- Standardized Binance order validations with proper order denied events to avoid "hanging" orders
- Refined Renko bar aggregator and add tests (#2961), thanks @faysou
- Refined setting of flags in Makefile (#3060), thanks @faysou
- Refined Bybit balance parsing to use `Money.from_str` to ensure no rounding errors
- Refined Interactive Brokers execution flows (#2993), thanks @faysou
- Refined Interactive Brokers filtering of bars in IB adapter after disconnection (#3011), thanks @faysou and @Johnkhk
- Refined Interactive Brokers account summary log to debug level (#3084), thanks @sunlei
- Refined catalog `reset_data_file_names` method (#3071), thanks @adrianbeer and @faysou
- Optimized `ExecutionEngine` hot path with topic caching and reduced cache lookups
- Optimized rate limiter quota keys with string interning to avoid repeated allocations
- Upgraded Rust (MSRV) to 1.90.0
- Upgraded Cython to v3.1.6
- Upgraded `databento` crate to v0.35.0
- Upgraded `datafusion` crate to v50.3.0
- Upgraded `pyo3` and `pyo3-async-runtimes` crates to v0.26.0
- Upgraded `redis` crate to v0.32.7
- Upgraded `tokio` crate to v1.48.0
- Upgraded `uvloop` to v0.22.1 (upgrades libuv to v1.49.0)

### Documentation Updates
- Added quick-reference rate limit tables with links to official docs for Binance, Bybit, OKX, BitMEX, and Coinbase International
- Updated cache concept guide with purging ops
- Improved dark and light themes for readability
- Improved clarity of implemented bar aggregations
- Standardized consistent styling per docs style guide
- Fixed some broken links

### Deprecations
- Deprecated `convert_quote_qty_to_base`; disable (`False`) to maintain consistent behaviour going forwards. Automatic conversion will be removed in a future version.

---

# NautilusTrader 1.220.0 Beta

Released on 9th September 2025 (UTC).

### Enhancements
- Added initial BitMEX integration adapter
- Added `FundingRateUpdate` data type with caching support through data engine
- Added `subscribe_funding_rates(...)` and `unsubscribe_funding_rates(...)` methods for actors
- Added `on_funding_rate(...)` handler for actors
- Added `funding_rate(...)` and `add_funding_rate(...)` for `Cache`
- Added `due_post_only` field for `OrderRejected` event, only properly populated for Binance and Bybit for now
- Added `log_rejected_due_post_only_as_warning` config option for `StrategyConfig` (default `True` to retain current behavior)
- Added `log_rejected_due_post_only_as_warning` config option for `BinanceExecClientConfig` (default `True` to retain current behavior)
- Added `log_components_only` config option for Logger (#2931), thanks @faysou
- Added support for additional Databento schemas: `CMBP_1`, `CBBO_1S`, `CBBO_1M`, `TCBBO`, and `OHLCV_EOD`
- Added configurable schema parameters for Databento quote and trade subscriptions, allowing `TBBO`/`TCBBO` for efficient combined data feeds
- Added support for option combos for Interactive Brokers (#2812), thanks @faysou
- Added support for execution of option spreads in backtesting (#2853), thanks @faysou
- Added support for option spread quotes in backtest (#2845), thanks @faysou
- Added loading of options chain from `request_instruments` for Interactive Brokers (#2809), thanks @faysou
- Added `OptionExerciseModule` (#2907), thanks @faysou
- Added `MarginModel` concept, base models, config, and factory for backtesting (#2794), thanks @faysou and @stefansimik
- Added additional built-in backtest fill models (#2795), thanks @faysou and @stefansimik
- Added `OrderBookDepth10DataWrangler` (#2801), thanks @trylovetom
- Added `group_size` parameter for PyO3 `OrderBook.pprint(...)` and `OwnOrderBook.pprint(...)`
- Added custom error logging function support for `RetryManager`
- Added Bybit options support (#2821), thanks @Baerenstein
- Added Bybit `is_leverage` order parameter support
- Added `persist_account_events` config option for `CacheConfig` (default `True` to retain current behavior)
- Added `query_account` method for `Strategy`
- Added `QueryAccount` execution message
- Added streaming methods for `TardisCSVDataLoader`
- Added stream iterators support for `BacktestEngine` low-level streaming API
- Added `YEAR` aggregation and improved bar specification validation (#2771), thanks @stastnypremysl
- Added support for requesting any number of historical bars for dYdX (#2766, #2777), thanks @DeirhX
- Added `use_hyphens_in_client_order_ids` config option for `StrategyConfig`
- Added `greeks_filter` function to `portfolio_greeks` (#2756), thanks @faysou
- Added time weighted and percent vega for `GreeksCalculator` (#2817), thanks @faysou
- Added `VERBOSE` option to common make targets (#2759), thanks @faysou
- Added bulk key loading capability for Redis cache database adapter
- Added `multiplier` field for `CurrencyPair` instrument (required for some crypto pairs)
- Added `tick_scheme_name` field for instrument dictionary conversions
- Added default `FixedTickScheme`(s) for all valid precisions
- Added PancakeSwapV3 pool parsing (#2829), thanks @filipmacek
- Added `PortfolioConfig.min_account_state_logging_interval_ms` config option for throttling account state logging
- Added `allow_cash_borrowing` config option for `BacktestVenueConfig` to enable negative balances in cash accounts
- Added borrowing support for Bybit SPOT accounts, enabling margin trading with negative balances
- Added initial DEX Pool filtering configuration (#2842, #2887), thanks @filipmacek
- Added Arbitrum FluidDEX pool parsing (#2897), thanks @filipmacek
- Added a complete `.env.example` template to guide environment configuration (#2877), thanks @nicolad
- Added Interactive Brokers OCA setting to order groups (#2899), thanks @faysou
- Added Interactive Brokers subscriptions for position updates (#2887), thanks @faysou
- Added `avg_px_open` field to `PositionStatusReport` for IB adapter (#2925), thanks @dinana
- Added support for running separate live and paper IB Gateway containers simultaneously (#2937), thanks @Bshara23
- Added support for data deduplication on catalog consolidation (#2934), thanks @ms32035

### Breaking Changes
- Added `multiplier` field for `CurrencyPair` Arrow schema
- Changed `start` parameter to required for `Actor` data request methods
- Reverted implementation of `delete_account_event` from cache database that was too inefficient and is now a no-op pending redesign
- Renamed `ParquetDataCatalog.reset_catalog_file_names` to `reset_all_file_names`
- Renamed `BinanceAccountType.USDT_FUTURE` to `USDT_FUTURES` for more conventional terminology
- Renamed `BinanceAccountType.COIN_FUTURE` to `COIN_FUTURES` for more conventional terminology
- Renamed `InstrumentMiniInfo` to `TardisInstrumentMiniInfo` to standardize adapter naming conventions
- Removed the generic `cvec_drop` FFI function, as it was unused and prone to misuse, potentially causing memory leaks
- Removed redundant `managed` parameter for `Actor.subscribe_book_at_interval` (the book *must* be managed by the `DataEngine` to provide snapshots at intervals)
- Consolidated `OwnBook` `group_bids` and `group_asks` methods into `bid_quantity` and `ask_quantity` with optional `depth` and `group_size` parameters
- Consolidated ~40 individual indicator modules into 6 files to reduce binary size
- Consolidated `backtest.exchange` into `backtest.engine` to reduce binary size
- Consolidated `backtest.matching_engine` into `backtest.engine` to reduce binary size
- Changed indicator imports from nested modules to flat structure (e.g., `from nautilus_trader.indicators.atr import AverageTrueRange` becomes `from nautilus_trader.indicators import AverageTrueRange`)
- Changed `NAUTILUS_CATALOG_PATH` to `NAUTILUS_PATH` for Tardis adapter (#2850), thanks @nicolad
- Simplified Binance environment variables for API credentials: removed separate variables for RSA/Ed25519 keys and consolidated mainnet spot/futures credentials
- Moved `Indicator` base class from `nautilus_trader.indicators.base.indicator` to `nautilus_trader.indicators.base`

### Internal Improvements
- Refactored OKX adapter to Rust API clients
- Refactored `BacktestDataIterator` (#2791) to consolidate data generator usage, thanks @faysou
- Implemented `LogGuard` reference counting for proper thread lifecycle management, ensuring all logs flushed before termination
- Implemented live subscriptions for blockchain data client (#2832), thanks @filipmacek
- Implemented initial Hyperliquid adapter (#2912, #2916, #2922, #2935), thanks @nicolad
- Introduced `SharedCell` / `WeakCell` wrappers for ergonomic and safer handling of `Rc<RefCell<T>>` / `Weak<RefCell<T>>` pairs
- Introduced efficient block syncing command in the `nautilus-cli` (#2861), thanks @filipmacek
- Introduced pool events syncing command in blockchain data client (#2920), thanks @filipmacek
- Added stream iterators support `BacktestDataIterator`
- Added serialization support for execution reports
- Added serialization support for execution report commands
- Added `DataTester` standardized data testing actor for integration adapters
- Added `start` and `stop` to response data (#2748), thanks @stastnypremysl
- Added integration test service management targets (#2765), thanks @stastnypremysl
- Added integration tests for dYdX bar-partitioning and large-history handling (#2773), thanks @nicolad
- Added make build-debug-pyo3 (#2802), thanks @faysou
- Added pytest timer (#2834), thanks @faysou
- Added support for several instrument versions with `request_instrument` (#2835), thanks @faysou
- Added `_send_position_status_report` to base execution client (#2926), thanks @faysou
- Added `passthrough_bar_type` to `TimeBarAggregator` (#2929), thanks @faysou
- Added matching engine check to return early if `last_qty` is non-positive (#2930), thanks @GhostLee
- Added `avg_px` population in order filled events for Interactive Brokers adapter (#2938), thanks @dinana
- Optimized identifiers hashing to avoid frequent recomputations using C strings
- Optimized data engine topic string caching for message bus publishing to avoid frequent f-string constructions
- Optimized Redis key scans to improve efficiency over a network
- Completed bar request implementation for OKX (#2789), thanks @nicolad
- Continued `ExecutionEngine` and testing in Rust (#2886), thanks @dakshbtc
- Enabled parallel pytest tests with `pytest-xdist` (#2808), thanks @stastnypremysl
- Standardized DeFi chain name validation for `InstrumentId` (#2826), thanks @filipmacek
- Standardized `NAUTILUS_PATH` env var across Tardis integration (#2850), thanks @nicolad
- Standardized zero PnL as Money instead of None when exchange rate missing (#2880), thanks @nicolad
- Refactored `SpreadQuoteAggregator` (#2905), thanks @faysou
- Refactored bar aggregators to use `ts_init` instead of `ts_event` (#2924), thanks @faysuo
- Improved typing for all the DEX IDs with `DexType` and add validation (#2827), thanks @filipmacek
- Improved reconciliation handling of internally generated orders to align positions (now uses the `INTERNAL-DIFF` strategy ID)
- Improved data client for blockchain adapter (#2787), thanks @filipmacek
- Improved DEX pool sync process in the blockchain adapter (#2796), thanks @filipmacek
- Improved efficiency of message bus external streams buffer flushing
- Improved `databento_test_request_bars` example (#2762), thanks @faysou
- Improved zero-sized trades handling for Tardis CSV loader (will log a warning)
- Improved ergonomics of `TardisInstrumentProvider` datetime filter params (can be either `pd.Timestamp` or Unix nanos `int`)
- Improved handling of Tardis Machine websocket connection errors
- Improved positions report to mark snapshots (#2840), thanks @stastnypremysl
- Improved ERC20 token metadata handling and error recovery (#2847), thanks @filipmacek
- Improved Docker configuration (#2868), thanks @nicolad
- Improved security for `Credential` struct (#2882), thanks @nicolad
- Improved DeFi pool event parsing and integrate Arbitrum Camelotv3 new pools signature (#2889), thanks @filipmacek
- Improved Databento multiplier decoding to prevent precision loss (#2895), thanks @nicolad
- Improved Bybit balance precision by avoiding float conversion (#2903), thanks @scoriiu
- Improved dYdX message parsing robustness to allow unknown fields (#2911), thanks @davidsblom
- Improved Polymarket instrument provider bulk loading (#2913), thanks @DeirhX
- Improved Polymarket binary options parsing with no `endDate` (#2919), thanks @DeirhX
- Refined Rust catalog path handling (#2743), thanks @faysou
- Refined Rust `GreeksCalculator` (#2760), thanks @faysou
- Refined Databento bars timestamp decoding and backtest execution usage (#2800), thanks @faysou
- Refined allowed queries for bars from `BacktestDataConfig` (#2838), thanks @faysou
- Refined `FillModel` (#2795), thanks @faysou and @stefansimik
- Refined request of instruments (#2822), thanks @faysou
- Refined `subscribe_bars` in IB adapter (#2852), thanks @faysou
- Refined `get_start_time` in `TimeBarAggregator` (#2866), thanks @faysou
- Refined option spread execution (#2859), thanks @faysou
- Refined `subscribe_historical_bars` in IB adapter (#2870), thanks @faysou
- Relaxed conditions on `start` and `end` of instrument requests in adapters (#2867), thanks @faysou
- Updated `request_aggregated_bars` example (#2815), thanks @faysou
- Updated PostgreSQL connection parameters to use 'nautilus' user (#2805), thanks @stastnypremysl
- Upgraded Rust (MSRV) to 1.89.0
- Upgraded Cython to v3.1.3
- Upgraded `web3` for Polymarket allowances script (#2814), thanks @DeirhX
- Upgraded `databento` crate to v0.33.1
- Upgraded `datafusion` crate to v49.0.1
- Upgraded `redis` crate to v0.32.5
- Upgraded `tokio` crate to v1.47.1

### Fixes
- Fixed Rust-Python reference cycles by replacing `Arc<PyObject>` with plain `PyObject` in callback-holding structs, eliminating memory leaks
- Fixed `TimeEventHandler` memory leaks with Python callback references in FFI layer
- Fixed `PyCapsule` memory leaks by adding destructors to enable proper Rust value cleanup
- Fixed multiple circular-dependency memory leaks for network and bar Python callbacks using new `SharedCell`/`WeakCell` helpers
- Fixed precision preservation for value types (`Price`, `Quantity`, `Money`)
- Fixed incorrect raw price type for matching engine in high-precision mode that could overflow during trades processing (#2810), thanks for reporting @Frzgunr1 and @happysammy
- Fixed incorrect currency used for cash account SELL orders pre-trade risk check
- Fixed accounting for locked balance with multiple currencies (#2918), thanks @GhostLee
- Fixed portfolio realized PnL for NETTING OMS position snapshot cycles (#2856), thanks for reporting @idobz and analysis @paulbir
- Fixed decoding zero-sized trades for Databento MBO data
- Fixed purging of contingent orders where open linked orders would still be purged
- Fixed backtest bracket order quantity independence, preventing child orders from incorrectly syncing to net position size
- Fixed Tardis Machine replay processing and Parquet file writing
- Fixed Tardis exchange-venue mapping for Kraken Futures (should map to `cryptofacilities`)
- Fixed Tardis CSV loader for book snapshots with interleaved bid/ask columns
- Fixed Polymarket reconciliation for signature type 2 trades where wallet address differs from funder address
- Fixed catalog query of multiple instruments of same type (#2772), thanks @faysou
- Fixed modification of contingent orders in backtest (#2761), thanks faysou
- Fixed balance calculations on order fill to allow operating at near account balance capacity (#2752), thanks @petioptrv
- Fixed cash account locked balance calculations for sell orders (#2906), thanks for reporting @GhostLee
- Fixed time range end in some databento request functions (#2755), thanks @faysou
- Fixed `skip_first_non_full_bar` tolerance for near-boundary starts (#2605), thanks for reporting @stastnypremysl
- Fixed EOD bar for Interactive Brokers (#2764), thanks @faysou
- Fixed dYdX Take Profit order type mapping error (#2758), thanks @nicolad
- Fixed dYdX logging typo (#2790), thanks @DeirhX
- Fixed dYdX order and fill message schemas (#2824), thanks @davidsblom
- Fixed dYdX message schemas (#2910), thanks @davidsblom
- Fixed Binance Spot testnet streaming URL, thanks for reporting @Frzgunr1
- Fixed Binance US trading fee endpoint URL (#2914), thanks for reporting @bmlquant
- Fixed Binance Ed25519 key handling
- Fixed Bybit execution fee handling where the `execFee` field was not used when available as well as incorrect fee currency
- Fixed Bybit instrument provider fee rate handling during parsing
- Fixed Bybit SPOT commission currency for makers
- Fixed Bybit positions pagination to handle more than 20 positions (#2879), thanks @scoriiu
- Fixed Bybit REST model parsing balance precision errors for high-value tokens (#2898), thanks @scoriiu
- Fixed Bybit WebSocket message parsing balance precision errors for high-value tokens (#2904), thanks @scoriiu
- Fixed OKX bars request pagination logic (#2798, #2825), thanks @nicolad
- Fixed RPC client content type header (#2828), thanks @filipmacek
- Fixed `venue_order_id` handling for Polymarket order status request (#2848), thanks @DeirhX
- Fixed race-condition on node shutdown in async `InteractiveBrokersDataClient._disconnect()` (#2865), thanks @ruvr
- Fixed `AttributeError` when loading cached `IBContract` objects (#2862), thanks @ruvr
- Fixed `PolymarketUserTrade.bucket_index` field type that changed from `str` to `int` (#2872), thanks for reporting @thefabus
- Fixed Polymarket websocket 500 tokens per connection limitation (#2915), thanks @odobias and @DeirhX
- Fixed Interactive Brokers `submit_order_list` rejection (#2892), thanks @faysou
- Fixed Interactive Brokers bars query for indices (#2921), thanks @ms32035
- Fixed missing `funding_rates` for Cache Debug impl (#2894), thanks @MK27MK
- Fixed missing `log_component_levels` for PyO3 logging initialization
- Fixed catalog consolidation name clash for an overlapping edge case (#2933), thanks @ms32035
- Fixed historical data request race condition in DataEngine (#2946), thanks @lisiyuan656
- Fixed catalog metadata retention on deduplication (#2943), thanks @ms32035

### Documentation Updates
- Added Positions concept guide
- Added Reports concept guide
- Added FFI Memory Contract developer guide
- Added Windows signal handling guidance
- Added mixed debugging instructions and example (#2806), thanks @faysou
- Improved dYdX integration guide (#2751), thanks @nicolad
- Updated IB documentation for option spreads (#2839), thanks @faysou
- Moved rust-python debugging documentation to `testing.md` (#2928), thanks @faysou

### Deprecations
None

---

# NautilusTrader 1.219.0 Beta

Released on 5th July 2025 (UTC).

### Enhancements
- Added `graceful_shutdown_on_exception` config option for live engines (default `False` to retain intended hard crash on unexpected system exceptions)
- Added `purge_from_database` config option for `LiveExecEngineConfig` to support cache backing database management
- Added support for data download during backtest (#2652), thanks @faysou
- Added delete data range to catalog (#2744), thanks @faysou
- Added consolidate catalog by period (#2727), thanks @faysou
- Added `fire_immediately` flag parameter for timers where a time event will be fired at the `start` instant and then every interval thereafter (default `False` to retain current behavior) (#2600), thanks for the idea @stastnypremysl
- Added `time_bars_build_delay` config option for `DataEngineConfig` (#2676), thanks @faysou
- Added immediate firing capability for time alerts and corresponding test (#2745), thanks @stastnypremysl
- Added missing serialization mappings for some instruments (#2702), thanks @faysou
- Added support for DEX swaps for blockchain adapter (#2683), thanks @filipmacek
- Added support for Pool liquidity updates for blockchain adapter (#2692), thanks @filipmacek
- Added fill report reconciliation warning when discrepancy with existing fill (#2706), thanks @faysou
- Added optional metadata function for custom data query (#2724), thanks @faysou
- Added support for order-list submission in the sandbox execution client (#2714), thanks @petioptrv
- Added hidden order support for IBKR (#2739), thanks @sunlei
- Added `subscribe_order_book_deltas` support for IBKR (#2749), thanks @sunlei
- Added `bid_levels` and `ask_levels` for `OrderBook.pprint`
- Added `accepted_buffer_ns` filter param for `Cache.own_bid_orders(...)` and `Cache.own_ask_orders(...)`
- Added trailing stop orders `activation_price` support in Rust (#2750), thanks @nicolad

### Breaking Changes
- Changed timer `allow_past=False` behavior: now validates the `next_event_time` instead of the `start_time`. This allows timers with past start times as long as their next scheduled event is still in the future
- Changed behavior of timers `allow_past=False` to permit start times in the past if the next event time is still in the future
- Changed Databento DBN upgrade policy to default v3
- Removed `basename_template` from `ParquetDataCatalog.write_data(...)`, run `catalog.reset_all_file_names()` to update file names to the new convention
- Removed problematic negative balance check for margin accounts (cash account negative balance check remains unchanged)
- Removed support for Databento DBN v1 schemas (migrate to DBN v2 or v3, see [DBN Changelog](https://github.com/databento/dbn/blob/main/CHANGELOG.md#0350---2025-05-28))

### Internal Improvements
- Added logging macros for custom component and color in Rust
- Added Cython-level parameter validation for timer operations to prevent Rust panics and provide clearer Python error messages
- Added property-based testing for `Price`, `Quantity`, `Money` value types in Rust
- Added property-based testing for `UnixNanos` in Rust
- Added property-based testing for `OrderBook` in Rust
- Added property-based testing for `TestTimer` in Rust
- Added property-based testing for `network` crate in Rust
- Added chaos testing with `turmoil` for socket clients in Rust
- Added `check_positive_decimal` correctness function and use for instrument validations (#2736), thanks @nicolad
- Added `check_positive_money` correctness function and use for instrument validations (#2738), thanks @nicolad
- Ported data catalog refactor to Rust (#2681, #2720), thanks @faysou
- Optimized `TardisCSVDataLoader` performance (~90% memory usage reduction, ~60-70% faster)
- Consolidated the clocks and timers v2 feature from @twitu
- Consolidated on pure Rust cryptography crates with no dependencies on native certs or openssl
- Consolidated on `aws-lc-rs` cryptography for FIPS compliance
- Confirmed parity between Cython and Rust indicators (#2700, #2710, #2713), thanks @nicolad
- Implemented `From<Pool>` -> `CurrencyPair` & `InstrumentAny` (#2693), thanks @nicolad
- Updated `Makefile` to use new docker compose syntax (#2746), thanks @stastnypremysl
- Updated Tardis exchange mappings
- Improved live engine message processing to ensure unexpected exceptions result in an immediate hard crash rather than continuing without the queue processing messages
- Improved live reconciliation robustness and testing
- Improved listen key error handling and recovery for Binance
- Improved handling of negative balances in backtests (#2730), thanks @ms32035
- Improved robustness of cash and margin account locked balance calculations to avoid negative free balance
- Improved robustness of fill price parsing for Betfair
- Improved implementation, validations and testing for Rust instruments (#2723, #2733), thanks @nicolad
- Improved `Currency` equality to use `strcmp` to avoid C pointer comparison issues with `ustr` string interning
- Improved unsubscribe cleanup(s) for Bybit adapter
- Improved `Makefile` to be self-documenting (#2741), thanks @sunlei
- Refactored IB adapter (#2647), thanks @faysou
- Refactored data catalog (#2652, #2740), thanks @faysou
- Refined Rust data catalog (#2734), thanks @faysou
- Refined logging subsystem lifecycle management and introduce global log sender
- Refined signal serialization and tests (#2705), thanks @faysou
- Refined CI/CD and build system (#2707), thanks @stastnypremysl
- Upgraded Rust (MSRV) to 1.88.0
- Upgraded Cython to v3.1.2
- Upgraded `databento` crate to v0.28.0
- Upgraded `datafusion` crate to v48.0.0
- Upgraded `pyo3` and `pyo3-async-runtimes` crates to v0.25.1
- Upgraded `redis` crate to v0.32.3
- Upgraded `tokio` crate to v1.46.1
- Upgraded `tokio-tungstenite` crate to v0.27.0

### Fixes
- Fixed `AccountBalance` mutation in `AccountState` events (#2701), thanks for reporting @DeirhX
- Fixed order book cache consistency in update and remove operations (found through property-based testing)
- Fixed order status report generation for Polymarket where `venue_order_id` was unbounded
- Fixed data request identifier attribute access for `LiveDataClient`
- Fixed `generate_order_modify_rejected` typo in Binance execution client (#2682), thanks for reporting @etiennepar
- Fixed order book depth handling in subscriptions for Binance
- Fixed potential `IndexError` with empty bars requests for Binance
- Fixed GTD-GTC time in force conversion for Binance
- Fixed incorrect logging of trigger type for Binance
- Fixed trade ticks unsubscribe for Binance which was not differentiating aggregated trades
- Fixed pending update hot cache cleanup for Betfair execution client
- Fixed invalid session information on account update for Betfair execution client
- Fixed order book snapshots unsubscribe for Tardis data client
- Fixed Arrow schema registration for `BinanceBar`
- Fixed gRPC server shutdown warning when running dYdX integration tests
- Fixed registration of encoder and decoder for `BinanceBar`, thanks for reporting @miller-moore
- Fixed spot and futures sandbox for Binance (#2687), thanks @petioptrv
- Fixed `clean` and `distclean` make targets entering `.venv` and corrupting the Python virtual env, thanks @faysou
- Fixed catalog identifier matching to exact match (#2732), thanks @faysou
- Fixed last value updating for RSI indicator (#2703), thanks @bartlaw
- Fixed gateway/TWS reconnect process for IBKR (#2710), thanks @bartlaw
- Fixed Interactive Brokers options chain issue (#2711), thanks @FGU1
- Fixed Partially filled bracket order and SL triggered for IBKR (#2704, #2717), thanks @bartlaw
- Fixed instrument message decoding when no `exchange` value for Databento US equities
- Fixed fetching single-instrument trading fees for `Binance`, thanks @petioptrv
- Fixed IB-TWS connection issue with international languages (#2726), thanks @DracheShiki
- Fixed bar requests for Bybit where pagination was incorrect which limited bars being returned
- Fixed Bybit Unknown Error (#2742), thanks @DeevsDeevs
- Fixed margin balance parsing for Bybit
- Restored task error logs for IBKR (#2716), thanks @bartlaw

### Documentation Updates
- Updated IB adapter documentation (#2729), thanks @faysou
- Improved reconciliation docs in live concept guide

### Deprecations
- Deprecated `Portfolio.set_specific_venue(...)`, to be removed in a future release; use `Cache.set_specific_venue(...)` instead

---

# NautilusTrader 1.218.0 Beta

Released on 31st May 2025 (UTC).

### Enhancements
- Added convenient re-exports for Betfair adapter (constants, configs, factories, types)
- Added convenient re-exports for Binance adapter (constants, configs, factories, loaders, types)
- Added convenient re-exports for Bybit adapter (constants, configs, factories, loaders, types)
- Added convenient re-exports for Coinbase International adapter (constants, configs, factories)
- Added convenient re-exports for Databento adapter (constants, configs, factories, loaders, types)
- Added convenient re-exports for dYdX adapter (constants, configs, factories)
- Added convenient re-exports for Polymarket adapter (constants, configs, factories)
- Added convenient re-exports for Tardis adapter (constants, configs, factories, loaders)
- Added support for `FillModel`, `LatencyModel` and `FeeModel` in BacktestNode (#2601), thanks @faysou
- Added bars caching from `request_aggregated_bars` (#2649), thanks @faysou
- Added `BacktestDataIterator` to backtest engine to provide on-the-fly data loading (#2545), thanks @faysou
- Added support for `MarkPriceUpdate` streaming from catalog (#2582), thanks @bartolootrit
- Added support for Binance Futures margin type (#2660), thanks @bartolootrit
- Added support for Binances mark price stream across all markets (#2670), thanks @sunlei
- Added `bars_timestamp_on_close` config option for Databento which defaults to `True` to consistently align with Nautilus conventions
- Added `activation_price` support for trailing stop orders (#2610), thanks @hope2see
- Added trailing stops for OrderFactory bracket orders (#2654), thanks @hope2see
- Added `raise_exception` config option for `BacktestRunConfig` (default `False` to retain current behavior) which will raise exceptions to interrupt a nodes run process
- Added `UnixNanos::is_zero()` convenience method to check for a zero/epoch value
- Added SQL schema, model, and query for `OrderCancelRejected`
- Added SQL schema, model, and query for `OrderModifyRejected`
- Added HyperSync client to blockchain adapter (#2606), thanks @filipmacek
- Added support for DEXs, pools, and tokens to blockchain adapter (#2638), thanks @filipmacek

### Breaking Changes
- Changed trailing stops to use `activation_price` rather than `trigger_price` for Binance to more closely match the Binance API conventions

### Internal Improvements
- Added `activation_price` str and repr tests for trailing stop orders (#2620), thanks @hope2see
- Added condition check for order `contingency_type` and `linked_order_ids` where a contingency should have associated linked order IDs
- Improved robustness of socket client reconnects and disconnects to avoid state race conditions
- Improved error handling for socket clients, will now raise Python exceptions on send errors rather than logging with `tracing` only
- Improved error handling for Databento adapter by changing many unwraps to instead log or raise Python exceptions (where applicable)
- Improved error handling for Tardis adapter by changing many unwraps to instead log or raise Python exceptions (where applicable)
- Improved fill behavior for limit orders in `L1_MBP` books, will now fill entire size when marketable as `TAKER` or market moves through limit as `MAKER`
- Improved account state event generation for margin accounts, avoiding the generation of redundant intermediate account states for the same execution event
- Improved ergonomics of messaging topics, patterns, and endpoints in Rust (#2658), thanks @twitu
- Improved development debug builds with cranelift backend for Rust (#2640), thanks @twitu
- Improved validations for `LimitOrder` in Rust (#2613), thanks @nicolad
- Improved validations for `LimitIfTouchedOrder` in Rust (#2533), thanks @nicolad
- Improved validations for `MarketIfTouchedOrder` in Rust (#2577), thanks @nicolad
- Improved validations for `MarketToLimitOrder` in Rust (#2584), thanks @nicolad
- Improved validations for `StopLimitOrder` in Rust (#2593), thanks @nicolad
- Improved validations for `StopMarketOrder` in Rust (#2596), thanks @nicolad
- Improved validations for `TrailingStopMarketOrder` in Rust (#2607), thanks @nicolad
- Improved orders initialize and display tests in Rust (#2617), thanks @nicolad
- Improved testing for Rust orders module (#2578), thanks @dakshbtc
- Improved Cython-Rust indicator parity for `AdaptiveMovingAverage` (AMA) (#2626), thanks @nicolad
- Improved Cython-Rust indicator parity for `DoubleExponentialMovingAverage` (DEMA) (#2633), thanks @nicolad
- Improved Cython-Rust indicator parity for `ExponentialMovingAverage` (EMA) (#2642), thanks @nicolad
- Improved Cython-Rust indicator parity for `HullMovingAverage` (HMA) (#2648), thanks @nicolad
- Improved Cython-Rust indicator parity for `LinearRegression` (#2651), thanks @nicolad
- Improved Cython-Rust indicator parity for `WilderMovingAverage` (RMA) (#2653), thanks @nicolad
- Improved Cython-Rust indicator parity for `VariableIndexDynamicAverage` (VIDYA) (#2659), thanks @nicolad
- Improved Cython-Rust indicator parity for `SimpleMovingAverage` (SMA) (#2655), thanks @nicolad
- Improved Cython-Rust indicator parity for `VolumeWeightedAveragePrice` (VWAP) (#2661), thanks @nicolad
- Improved Cython-Rust indicator parity for `WeightedMovingAverage` (WMA) (#2662), thanks @nicolad
- Improved Cython-Rust indicator parity for `ArcherMovingAveragesTrends` (AMAT) (#2669), thanks @nicolad
- Improved zero size trade logging for Binance Futures (#2588), thanks @bartolootrit
- Improved error handling on API key authentication errors for Polymarket
- Improved execution client debug logging for Polymarket
- Improved exception on deserializing order from cache database
- Improved `None` condition checks for value types, which now raise a `TypeError` instead of an obscure `AttributeError`
- Changed `VecDeque` for fixed-capacity `ArrayDeque` in SMA indicator (#2666), thanks @nicolad
- Changed `VecDeque` for fixed-capacity `ArrayDeque` in LinearRegression (#2667), thanks @nicolad
- Implemented remaining Display for orders in Rust (#2614), thanks @nicolad
- Implemented `_subscribe_instrument` for dYdX and Bybit (#2636), thanks @davidsblom
- Untangled `ratelimiter` quota from `python` flag (#2595), thanks @twitu
- Refined `BacktestDataIterator` correctness (#2591), thanks @faysou
- Refined formatting of IB adapter files (#2639), thanks @faysou
- Optimized message bus topic-matching logic in Rust by 100× (#2634), thanks @twitu
- Changed to faster message bus pattern matching logic from Rust (#2643), thanks @twitu
- Upgraded Rust (MSRV) to 1.87.0
- Upgraded Cython to v3.1.0 (now stable)
- Upgraded `databento` crate to v0.26.0
- Upgraded `datafusion` crate to v48.0.2
- Upgraded `redis` crate to v0.31.0
- Upgraded `sqlx` crate to v0.8.6
- Upgraded `tokio` crate to v1.45.1

### Fixes
- Fixed portfolio account updates leading to incorrect balances (#2632, #2637), thanks for reporting @bartolootrit and @DeirhX
- Fixed portfolio handling of `OrderExpired` events not updating state (margin requirements may change)
- Fixed event handling for `ExecutionEngine` so it fully updates the `Portfolio` before to publishing execution events (#2513), thanks for reporting @stastnypremysl
- Fixed PnL calculation for margin account on position flip (#2657), thanks for reporting @Egisess
- Fixed notional value pre-trade risk check when order using quote quantity (#2628), thanks for reporting @DeevsDeevs
- Fixed position snapshot cache access for `ExecutionEngine`
- Fixed position snapshot `SystemError` calling `copy.deepcopy()` by simply using a `pickle` round trip to copy the position instance
- Fixed event purging edge cases for account and position where at least one event must be guaranteed
- Fixed authentication for Redis when password provided with no username
- Fixed various numpy and pandas FutureWarning(s)
- Fixed sockets exponential backoff immediate reconnect value on reset (this prevented immediate reconnects on the next reconnect sequence)
- Fixed message bus subscription matching logic in Rust (#2646), thanks @twitu
- Fixed trailing stop market fill behavior when top-level exhausted to align with market orders (#2540), thanks for reporting @stastnypremysl
- Fixed stop limit fill behavior on initial trigger where the limit order was continuing to fill as a taker beyond available liquidity, thanks for reporting @hope2see
- Fixed matching engine trade processing when aggressor side is `NO_AGGRESSOR` (we can still update the matching core)
- Fixed modifying and updating trailing stop orders (#2619), thanks @hope2see
- Fixed processing activated trailing stop update when no trigger price, thanks for reporting @hope2see
- Fixed terminating backtest on `AccountError` when streaming, the exception needed to be reraised to interrupt the streaming of chunks (#2546), thanks for reporting @stastnypremysl
- Fixed HTTP batch order operations for Bybit (#2627), thanks @sunlei
- Fixed `reduce_only` attribute access in batch place order for Bybit
- Fixed quote tick parsing for one-sided books on Polymarket
- Fixed order fill handling for limit orders with `MAKER` liquidity side on Polymarket
- Fixed currency parsing for `BinaryOption` on Polymarket to consistently use USDC.e (PoS USDC on Polygon)
- Fixed identity error handling during keep-alive for Betfair, will now reconnect
- Updated `BinanceFuturesEventType` enum with additional variants, thanks for reporting @miller-moore

### Documentation Updates
- Added capability matrices for integration guides
- Added content to Architecture concept guide
- Added content to Live Trading concept guide
- Added content to Developer Guide
- Added errors and panics docs for most crates
- Added errors and panics docs for most crates
- Improved the clarity of various concept guides
- Fixed several errors in concept guides

### Deprecations
- Deprecated support for Databento [instrument definitions](https://databento.com/docs/schemas-and-data-formats/instrument-definitions) v1 data, v2 & v3 continue to be supported and v1 data can be migrated (see Databento documentation)

---

# NautilusTrader 1.217.0 Beta

Released on 30th April 2025 (UTC).

### Enhancements
- Added processing of `OrderBookDepth10` for `BacktestEngine` and `OrderMatchingEngine` (#2542), thanks @limx0
- Added `Actor.subscribe_order_book_depth(...)` subscription method (#2555), thanks @limx0
- Added `Actor.unsubscribe_order_book_depth(...)` subscription method
- Added `Actor.on_order_book_depth(...)` handler method (#2555), thanks @limx0
- Added `UnixNanos::max()` convenience method for the maximum valid value
- Added `available_offset` filter parameter for `TardisInstrumentProvider`
- Added `NAUTILUS_WORKER_THREADS` environment variable for common tokio runtime builder
- Added `Quantity::non_zero(...)` method
- Added `Quantity::non_zero_checked(...)` method
- Added `round_down` param for `Instrument.make_qty(...)` that is `False` by default to maintain current behavior
- Added WebSocket batch order operations for Bybit (#2521), thanks @sunlei
- Added mark price subscription for Binance Futures (#2548), thanks @bartolootrit
- Added `Chain` struct to represent blockchain network (#2526), thanks @filipmacek
- Added `Block` primitive for blockchain domain model (#2535), thanks @filipmacek
- Added `Transaction` primitive for blockchain domain model (#2551), thanks @filipmacek
- Added initial blockchain adapter with live block subscription (#2557), thanks @filipmacek

### Breaking Changes
- Removed fees from locked balance calculations for `CASH` accounts
- Removed fees from margin calculations for `MARGIN` accounts
- Renamed `id` constructor parameter to `instrument_id` across all PyO3 instruments, aligning with equivalent Cython instrument constructors

### Internal Improvements
- Implemented exponential backoff and jitter for the `RetryManager` (#2518), thanks @davidsblom
- Simplified default locked balance and margin calculations to not include fees
- Improved handling of time range and effective date filters for `TardisInstrumentProvider`
- Improved reconnection robustness for Bybit private/trading channels (#2520), thanks @sunlei
- Improved logger buffers flushing post backtest
- Improved validations for Tardis trades data
- Improved correctness of client registration and deregistration for `ExecutionEngine`
- Improved build time by only compiling libraries (#2539), thanks @twitu
- Improved logging flush (#2568), thanks @faysou
- Improved `clear_log_file` to happen for each kernel initialization (#2569), thanks @faysou
- Refined `Price` and `Quantity` validations and correctness
- Filter fill events if order is already filled for dYdX (#2547), thanks @davidsblom
- Fixed some clippy lints (#2517), thanks @twitu
- Upgraded `databento` crate to v0.24.0
- Upgraded `datafusion` crate to v47.0.0
- Upgraded `redis` crate to v0.30.0
- Upgraded `sqlx` crate to v0.8.5
- Upgraded `pyo3` crate to v0.24.2

### Fixes
- Fixed consistent ordering of execution events (#2513, #2554), thanks for reporting @stastnypremysl
- Fixed type error when generating an elapsed time for backtests with no elapsed time
- Fixed memory leak in `RetryManager` by simplifying the acquire-release pattern, avoiding the asynchronous context manager protocol that led to state sharing, thanks for reporting @DeevsDeevs
- Fixed locked balance and initial margin calculations for reduce-only orders (#2505), thanks for reporting @stastnypremysl
- Fixed purging order events from position (these needed to be purged prior to removing cache index entry), thanks @DeevsDeevs
- Fixed `TypeError` when formatting backtest post run timestamps which were `None` (#2514), thanks for reporting @stastnypremysl
- Fixed handling of `BetfairSequenceCompleted` as custom data
- Fixed the instrument class of `IndexInstrument`, changing to `SPOT` to correctly represent a spot index of underlying constituents
- Fixed data range request `end` handling for `DataEngine`
- Fixed unsubscribe instrument close for `DataEngine`
- Fixed network clients authentication for OKX (#2553), thanks for reporting @S3toGreen
- Fixed account balance calculation for dYdX (#2563), thanks @davidsblom
- Fixed `ts_init` for databento historical data (#2566), thanks @faysou
- Fixed `RequestInstrument` in `query_catalog` (#2567), thanks @faysou
- Reverted removal of rotate log file on UTC date change (#2552), thanks @twitu

### Documentation Updates
- Improved environment setup guide with recommended rust analyzer settings (#2538), thanks @twitu
- Fixed alignment with code for some `ExecutionEngine` docstrings

### Deprecations
None

---

# NautilusTrader 1.216.0 Beta

Released on 13th April 2025 (UTC).

This release adds support for Python 3.13 (*not* yet compatible with free-threading),
and introduces support for Linux on ARM64 architecture.

### Enhancements
- Added `allow_past` boolean flag for `Clock.set_timer(...)` to control behavior with start times in the past (default `True` to allow start times in the past)
- Added `allow_past` boolean flag for `Clock.set_time_alert(...)` to control behavior with alert times in the past (default `True` to fire immediate alert)
- Added risk engine check for GTD order expire time, which will deny if expire time is already in the past
- Added instrument updating for exchange and matching engine
- Added additional price and quantity precision validations for matching engine
- Added log file rotation with additional config options `max_file_size` and `max_backup_count` (#2468), thanks @xingyanan and @twitu
- Added `bars_timestamp_on_close` config option for `BybitDataClientConfig` (default `True` to match Nautilus conventions)
- Added `BetfairSequenceCompleted` custom data type for Betfair to mark the completion of a sequence of messages
- Added Arrow schema for `MarkPriceUpdate` in Rust
- Added Arrow schema for `IndexPriceUpdate` in Rust
- Added Arrow schema for `InstrumentClose` in Rust
- Added `BookLevel.side` property
- Added `Position.closing_order_side()` instance method
- Improved robustness of in-flight order check for `LiveExecutionEngine`, once exceeded query retries will resolve submitted orders as rejected and pending orders as canceled
- Improved logging for `BacktestNode` crashes with full stack trace and prettier config logging

### Breaking Changes
- Changed external bar requests `ts_event` timestamping from on open to on close for Bybit

### Internal Improvements
- Added handling and warning for Betfair zero-sized fills
- Improved WebSocket error handling for dYdX (#2499), thanks @davidsblom
- Ported `GreeksCalculator` to Rust (#2493, #2496), thanks @faysou
- Upgraded Cython to v3.1.0b1
- Upgraded `redis` crate to v0.29.5
- Upgraded `tokio` crate to v1.44.2

### Fixes
- Fixed setting component clocks to backtest start time
- Fixed overflow error in trailing stop calculations
- Fixed missing `SymbolFilterType` enum member for Binance (#2495), thanks @sunlei
- Fixed `ts_event` for Bybit bars (#2502), thanks @davidsblom
- Fixed position ID handling for Binance Futures in hedging mode with execution algorithm order (#2504), thanks for reporting @Oxygen923

### Documentation Updates
- Removed obsolete bar limitations in portfolio docs (#2501), thanks @stefansimik

### Deprecations
None

---

# NautilusTrader 1.215.0 Beta

Released on 5th April 2025 (UTC).

### Enhancements
- Added `Cache.purge_closed_order(...)`
- Added `Cache.purge_closed_orders(...)`
- Added `Cache.purge_closed_position(...)`
- Added `Cache.purge_closed_positions(...)`
- Added `Cache.purge_account_events(...)`
- Added `Account.purge_account_events(...)`
- Added `purge_closed_orders_interval_mins` config option for `LiveExecEngineConfig`
- Added `purge_closed_orders_buffer_mins` config option for `LiveExecEngineConfig`
- Added `purge_closed_positions_interval_mins` config option for `LiveExecEngineConfig`
- Added `purge_closed_positions_buffer_mins` config option for `LiveExecEngineConfig`
- Added `purge_account_events_interval_mins` config option for `LiveExecEngineConfig`
- Added `purge_account_events_lookback_mins` config option for `LiveExecEngineConfig`
- Added `Order.ts_closed` property
- Added `instrument_ids` and `bar_types` for `BacktestDataConfig` to improve catalog query efficiency (#2478), thanks @faysou
- Added `venue_dataset_map` config option for `DatabentoDataConfig` to override the default dataset used for a venue (#2483, #2485), thanks @faysou

### Breaking Changes
None

### Internal Improvements
- Added `Position.purge_events_for_order(...)` for purging `OrderFilled` events and `TradeId`s associated with a client order ID
- Added `Consumer` for `WebSocketClient` (#2488), thanks @twitu
- Improved instrument parsing for Tardis with consistent `effective` timestamp filtering, settlement currency, increments and fees changes
- Improved error logging for Betfair `update_account_state` task by logging the full stack trace on error
- Improved logging for Redis cache database operations
- Standardized unexpected exception logging to include full stack trace
- Refined type handling for backtest configs
- Refined databento venue dataset mapping and configuration (#2483), thanks @faysou
- Refined usage of databento `use_exchange_as_venue` (#2487), thanks @faysou
- Refined time initialization of components in backtest (#2490), thanks @faysou
- Upgraded Rust (MSRV) to 1.86.0
- Upgraded `pyo3` crate to v0.24.1

### Fixes
- Fixed MBO feed handling for Databento where an initial snapshot was decoding a trade tick with zero size (#2476), thanks for reporting @JackWCollins
- Fixed position state snapshots for closed positions where these snapshots were being incorrectly filtered
- Fixed handling of `PolymarketTickSizeChanged` message
- Fixed parsing spot instruments for Tardis where `size_increment` was zero, now inferred from base currency
- Fixed default log colors for Rust (#2489), thanks @filipmacek
- Fixed sccache key for uv in CI (#2482), thanks @davidsblom

### Documentation Updates
- Clarified partial fills in backtesting concept guide (#2481), thanks @stefansimik

### Deprecations
- Deprecated strategies written in Cython and removed `ema_cross_cython` strategy example

---

# NautilusTrader 1.214.0 Beta

Released on 28th March 2025 (UTC).

### Enhancements
- Added [Coinbase International Exchange](https://www.coinbase.com/en/international-exchange) initial integration adapter
- Added `time_in_force` parameter for `Strategy.close_position(...)`
- Added `time_in_force` parameter for `Strategy.close_all_positions(...)`
- Added `MarkPriceUpdate` data type
- Added `IndexPriceUpdate` data type
- Added `Actor.subscribe_mark_prices(...)`
- Added `Actor.subscribe_index_prices(...)`
- Added `Actor.unsubscribe_mark_prices(...)`
- Added `Actor.unsubscribe_index_prices(...)`
- Added `Actor.on_mark_price(...)`
- Added `Actor.on_index_price(...)`
- Added `Cache.mark_price(...)`
- Added `Cache.index_price(...)`
- Added `Cache.mark_prices(...)`
- Added `Cache.index_prices(...)`
- Added `Cache.mark_price_count(...)`
- Added `Cache.index_price_count(...)`
- Added `Cache.has_mark_prices(...)`
- Added `Cache.has_index_prices(...)`
- Added `UnixNanos.to_rfc3339()` for ISO 8601 (RFC 3339) strings
- Added `recv_window_ms` config for Bybit WebSocket order client (#2466), thanks @sunlei
- Enhanced `UnixNanos` string parsing to support YYYY-MM-DD date format (interpreted as midnight UTC)

### Breaking Changes
- Changed `Cache.add_mark_price(self, InstrumentId instrument_id, Price price)` to `add_mark_price(self, MarkPriceUpdate mark_price)`

### Internal Improvements
- Improved `WebSocketClient` and `SocketClient` design with dedicated writer task and message channel
- Completed global message bus design in Rust (#2460), thanks @filipmacek
- Refactored enum dispatch (#2461), thanks @filipmacek
- Refactored data interfaces to messages in Rust
- Refined catalog file operations in Rust (#2454), thanks @faysou
- Refined quote ticks and klines for Bybit (#2465), thanks @davidsblom
- Standardized use of `anyhow::bail` (#2459), thanks @faysou
- Ported `add_venue` for `BacktestEngine` in Rust (#2457), thanks @filipmacek
- Ported `add_instrument` for `BacktestEngine` in Rust (#2469), thanks @filipmacek
- Upgraded `redis` crate to v0.29.2

### Fixes
- Fixed race condition on multiple reconnect attempts for `WebSocketClient` and `SocketClient`
- Fixed position state snapshot `ts_snapshot` value, which was always `ts_last` instead of timestamp when the snapshot was taken
- Fixed instrument parsing for Tardis, now correctly applies changes and filters by `effective`
- Fixed `OrderStatusReport` for conditional orders of dYdX (#2467), thanks @davidsblom
- Fixed submitting stop market orders for dYdX (#2471), thanks @davidsblom
- Fixed retrying HTTP calls on `DecodeError` for dYdX (#2472), thanks @davidsblom
- Fixed `LIMIT_IF_TOUCHED` order type enum parsing for Bybit
- Fixed `MARKET` order type enum parsing for Bybit
- Fixed quote ticks for Polymarket to only emit new quote ticks when the top-of-book changes
- Fixed error on cancel order for IB (#2475), thanks @FGU1

### Documentation Updates
- Improved custom data documentation (#2470), thanks @faysou

### Deprecations
None

---

# NautilusTrader 1.213.0 Beta

Released on 16th March 2025 (UTC).

### Enhancements
- Added `CryptoOption` instrument, supporting inverse and fractional sizes
- Added `Cache.prices(...)` to return a map of latest price per instrument for a price type
- Added `use_uuid_client_order_ids` config option for `StrategyConfig`
- Added catalog consolidation functions of several parquet files into one (#2421), thanks @faysou
- Added FDUSD (First Digital USD) crypto `Currency` constant
- Added initial leverage, `margin_mode` and `position_mode` config options for Bybit (#2441), thanks @sunlei
- Updated parquet catalog in Rust with recent features (#2442), thanks @faysou

### Breaking Changes
None

### Internal Improvements
- Added `timeout_secs` parameter to `HttpClient` for default timeouts
- Added additional precision validations for `OrderMatchingEngine`
- Added symmetric comparison impls between `u64` and `UnixNanos`
- Improved `InstrumentProvider` error handling when loading (#2444), thanks @davidsblom
- Improved order denied reason message for balance impact
- Handle BybitErrors when updating instruments for ByBit (#2437), thanks @davidsblom
- Handle unexpected errors when fetching order books for dYdX (#2445), thanks @davidsblom
- Retry if HttpError is raised for dYdX (#2438), thanks @davidsblom
- Refactored some Rust logs to use named parameters in format strings (#2443), thanks @faysou
- Some minor performance optimizations for Bybit and dYdX adapters (#2448), thanks @sunlei
- Ported backtest engine and kernel to Rust (#2449), thanks @filipmacek
- Upgraded `pyo3` and `pyo3-async-runtimes` crates to v0.24.0
- Upgraded `tokio` crate to v1.44.1

### Fixes
- Fixed source distribution (sdist) packaging
- Fixed `Clock.timer_names()` memory issue resulting in an empty list
- Fixed underflow panic when setting a time alert in the past (#2446), thanks for reporting @uxbux
- Fixed logger name for `Strategy` custom `strategy_id`s
- Fixed unbound variable for Bybit (#2433), thanks @davidsblom

### Documentation Updates
- Clarify docs for timestamp properties in `Data` (#2450), thanks @stefansimik
- Updated environment setup document (#2452), thanks @faysou

### Deprecations
None

---

# NautilusTrader 1.212.0 Beta

Released on 11th March 2025 (UTC).

This release introduces [uv](https://docs.astral.sh/uv) as the Python project and dependency management tool.

### Enhancements
- Added `OwnOrderBook` and `OwnBookOrder` to track own orders and prevent self-trades in market making
- Added `manage_own_order_books` config option for `ExecEngineConfig` to enable own order tracking
- Added `Cache.own_order_book(...)`, `Cache.own_bid_orders(...)` and `Cache.own_ask_orders(...)` for own order tracking
- Added optional beta weighting and percent option greeks (#2317), thanks @faysou
- Added pnl information to greeks data (#2378), thanks @faysou
- Added precision inference for `TardisCSVDataLoader`, where `price_precision` and `size_precision` are now optional
- Added `Order.ts_accepted` property
- Added `Order.ts_submitted` property
- Added `UnixNanos::to_datetime_utc()` in Rust
- Added `Mark` variant for `PriceType` enum
- Added mark price handling for `Cache`
- Added mark exchange rate handling for `Cache`
- Added `PortfolioConfig` for configuration settings specific to the `Portfolio`
- Added `use_mark_prices`, `use_mark_xrates` and `convert_to_account_base_currency` options for `PortfolioConfig`
- Added mark price calculations and xrate handling for `Portfolio`
- Added Rust debugging support and refined cargo nextest usage (#2335, #2339), thanks @faysou
- Added catalog write mode options (#2365), thanks @faysou
- Added `BarSpecification` to msgspec encoding and decoding hooks (#2373), thanks @pierianeagle
- Added `ignore_external_orders` config option for `BetfairExecClientConfig`, default `False` to retain current behavior
- Added requests for order book snapshots with HTTP for dYdX (#2393), thanks @davidsblom

### Breaking Changes
- Removed [talib](https://github.com/nautechsystems/nautilus_trader/tree/develop/nautilus_trader/indicators/ta_lib) subpackage (see deprecations for v1.211.0)
- Removed internal `ExchangeRateCalculator`, replaced with `get_exchange_rate(...)` function implemented in Rust
- Replaced `ForexSession` enum with equivalent from PyO3
- Replaced `ForexSessionFilter` with equivalent functions from PyO3
- Renamed `InterestRateData` to `YieldCurveData`
- Renamed `Cache.add_interest_rate_curve` to `add_yield_curve`
- Renamed `Cache.interest_rate_curve` to `yield_curve`
- Renamed `OrderBook.count` to `update_count` for clarity
- Moved `ExecEngineConfig.portfolio_bar_updates` config option to `PortfolioConfig.bar_updates`

### Internal Improvements
- Added initial `Cache` benchmarking for orders (#2341), thanks @filipmacek
- Added support for `CARGO_BUILD_TARGET` environment variable in `build.py` (#2385), thanks @sunlei
- Added test for time-bar aggregation (#2391), thanks @stefansimik and @faysou
- Implemented actor framework and message bus v3 (#2402), thanks @twitu
- Implemented latency modeling for SimulatedExchange in Rust (#2423), thanks @filipmacek
- Implemented exchange rate calculations in Rust
- Improved handling of `oms_type` for `StrategyConfig` which now correctly handles the `OmsType` enum
- Improved Binance websocket connections management to allow more than 200 streams (#2369), thanks @lidarbtc
- Improved log event timestamping to avoid clock or time misalignments when events cross to the logging thread
- Improved error logging for live engines to now include stacktrace for easier debugging
- Improved logging initialization error handling to avoid panicking in Rust
- Improved Redis cache database queries, serialization, error handling and connection management (#2295, #2308, #2318), thanks @Pushkarm029
- Improved validation for `OrderList` to check all orders are for the same instrument ID
- Improved `Controller` functionality with ability to create actors and strategies from configs (#2322), thanks @faysou
- Improved `Controller` creation for more streamlined trader registration, and separate clock for timer namespacing (#2357), thanks @faysou
- Improved build by adding placeholders to avoid unnecessary rebuilds (#2336), thanks @bartolootrit
- Improved consistency of `OrderMatchingEngine` between Cython and Rust and fix issues (#2350), thanks @filipmacek
- Removed obsolete reconnect guard for dYdX (#2334), thanks @davidsblom
- Refactored data request interfaces into messages (#2260), thanks @faysou
- Refactored data subscribe interfaces into messages (#2280), thanks @faysou
- Refactored reconciliation interface into messages (#2375), thanks @faysou
- Refactored `_handle_query_group` to work with `update_catalog` (#2412), thanks @faysou
- Refactored execution message handling in Rust (#2291), thanks @filipmacek
- Refactored repetitive code in backtest examples (#2387, #2395), thanks @stefansimik
- Refined yield curve data (#2300), thanks @faysou
- Refined bar aggregators in Rust (#2311), thanks @faysou
- Refined greeks computation (#2312), thanks @faysou
- Refined underlying filtering in portfolio_greeks (#2382), thanks @faysou
- Refined `request_instruments` granularity for Databento (#2347), thanks @faysou
- Refined Rust date functions (#2356), thanks @faysou
- Refined parsing of IB symbols (#2388), thanks @faysou
- Refined `base_template` behaviour in parquet write_data (#2389), thanks @faysou
- Refined mixed catalog client requests (#2405), thanks @faysou
- Refined update catalog docstring (#2411), thanks @faysou
- Refined to use `next_back` instead of `last` for identifier tag functions (#2414), thanks @twitu
- Refined and optimized `OrderBook` in Rust
- Cleaned up PyO3 migration artifacts (#2326), thanks @twitu
- Ported `StreamingFeatherWriter` to Rust (#2292), thanks @twitu
- Ported `update_limit_order` for `OrderMatchingEngine` in Rust (#2301), thanks @filipmacek
- Ported `update_stop_market_order` for `OrderMatchingEngine` in Rust (#2310), thanks @filipmacek
- Ported `update_stop_limit_order` for `OrderMatchingEngine` in Rust (#2314), thanks @filipmacek
- Ported market-if-touched order handling for `OrderMatchingEngine` in Rust (#2329), thanks @filipmacek
- Ported limit-if-touched order handling for `OrderMatchingEngine` in Rust (#2333), thanks @filipmacek
- Ported market-to-limit order handling for `OrderMatchingEngine` in Rust (#2354), thanks @filipmacek
- Ported trailing stop order handling for `OrderMatchingEngine` in Rust (#2366, #2376), thanks @filipmacek
- Ported contingent orders handling for `OrderMatchingEngine` in Rust (#2404), thanks @filipmacek
- Updated Databento `publishers.json` mappings file(s)
- Upgraded `nautilus-ibapi` to 10.30.1 with necessary changes for Interactive Brokers (#2420), thanks @FGU1
- Upgraded Rust to 1.85.0 and 2024 edition
- Upgraded `arrow` and `parquet` crates to v54.2.1
- Upgraded `databento` crate to v0.20.0 (upgrades the `dbn` crate to v0.28.0)
- Upgraded `datafusion` crate to v46.0.0
- Upgraded `pyo3` crate to v0.23.5
- Upgraded `tokio` crate to v1.44.0

### Fixes
- Fixed large difference between `Data` enum variants (#2315), thanks @twitu
- Fixed `start` and `end` range filtering for `TardisHttpClient` to use API query params
- Fixed built-in data type Arrow schemas for `StreamingFeatherWriter`, thanks for reporting @netomenoci
- Fixed memory allocation performance issue for `TardisCSVDataLoader`
- Fixed `effective` timestamp filtering for `TardisHttpClient` to now only retain latest version at or before `effective`
- Fixed contract `activation` for Binance Futures, now based on the `onboardDate` field
- Fixed hardcoded signature type for `PolymarketExecutionClient`
- Fixed unsubscribing from quotes for dYdX (#2331), thanks @davidsblom
- Fixed docstrings for dYdX factories (#2415), thanks @davidsblom
- Fixed incorrect type annotations in `_request_instrument` signature (#2332), thanks @faysou
- Fixed composite bars subscription (#2337), thanks @faysou
- Fixed sub command issue in some adapters (#2343), thanks @faysou
- Fixed `bypass_logging` fixture to keep log guard alive for entire test session
- Fixed time parsing for IB adapter (#2360), thanks @faysou
- Fixed bad `ts_init` value in IB weekly and monthly bar (#2355), thanks @Endura2024
- Fixed bar timestamps for IB (#2380), thanks @Endura2024
- Fixed backtest example load bars from custom CSV (#2383), thanks @hanksuper
- Fixed subscribe composite bars (#2390), thanks @faysou
- Fixed invalid link in IB docs (#2401), thanks @stefansimik
- Fixed cache index loading to ensure persisted data remains available after startup, thanks for reporting @Saransh-28
- Fixed bars pagination, ordering and limit for Bybit
- Fixed `update_bar` aggregation function to guarantee high and low price invariants (#2430), thanks @hjander and @faysou

### Documentation Updates
- Added documentation for messaging styles (#2410), thanks @stefansimik
- Added backtest clock and timers example (#2327), thanks @stefansimik
- Added backtest bar aggregation example (#2340), thanks @stefansimik
- Added backtest portfolio example (#2362), thanks @stefansimik
- Added backtest cache example (#2370), thanks @stefansimik
- Added backtest cascaded indicators example (#2398), thanks @stefansimik
- Added backtest custom event with msgbus example (#2400), thanks @stefansimik
- Added backtest messaging with msgbus example (#2406), thanks @stefansimik
- Added backtest messaging with actor & data example (#2407), thanks @stefansimik
- Added backtest messaging with actor & signal example (#2408), thanks @stefansimik
- Added indicators example (#2396), thanks @stefansimik
- Added documentation for debugging with Rust (#2325), thanks @faysou
- Added MRE strategy example (#2352), thanks @stefansimik
- Added data catalog example (#2353), thanks @stefansimik
- Improved and expandd bar aggregation docs (#2384), thanks @stefansimik
- Improved `emulation_trigger` parameter description in docstrings (#2313), thanks @stefansimik
- Improved docs for emulated orders (#2316), thanks @stefansimik
- Improved getting started doc for backtesting API levels (#2324), thanks @faysou
- Improved FSM example explanations for beginners (#2351), thanks @stefansimik
- Refined option greeks docstrings (#2320), thanks @faysou
- Refined adapters concept documentation (#2358), thanks @faysou
- Fixed typo in docs/concepts/actors.md (#2422), thanks @lsamaciel
- Fixed singular noun in docs/concepts/instruments.md (#2424), thanks @lsamaciel
- Fixed typo in docs/concepts/data.md (#2426), thanks @lsamaciel
- Fixed Limit-If-Touched example in docs/concepts/orders.md (#2429), thanks @lsamaciel

### Deprecations
None

---

# NautilusTrader 1.211.0 Beta

Released on 9th February 2025 (UTC).

This release introduces [high-precision mode](https://nautilustrader.io/docs/nightly/concepts/overview#value-types),
where value types such as `Price`, `Quantity` and `Money` are now backed by 128-bit integers (instead of 64-bit),
thereby increasing maximum precision to 16, and vastly expanding the allowable value ranges.

This will address precision and value range issues experienced by some crypto users, alleviate higher timeframe bar volume limitations, as well as future proofing the platform.

See the [RFC](https://github.com/nautechsystems/nautilus_trader/issues/2084) for more details.
For an explanation on compiling with or without high-precision mode, see the [precision-mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode) section of the installation guide.

**For migrating data catalogs due to the breaking changes, see the [data migrations guide](https://nautilustrader.io/docs/nightly/concepts/data#data-migrations)**.

**This release will be the final version that uses Poetry for package and dependency management.**

### Enhancements
- Added `high-precision` mode for 128-bit integer backed value types (#2072), thanks @twitu
- Added instrument definitions range requests for `TardisHttpClient` with optional `start` and `end` filter parameters
- Added `quote_currency`, `base_currency`, `instrument_type`, `contract_type`, `active`, `start` and `end` filters for `TardisInstrumentProvider`
- Added `log_commands` config option for `ActorConfig`, `StrategyConfig`, `ExecAlgorithmConfig` for more efficient log filtering
- Added additional limit parameters for `BettingInstrument` constructor
- Added `venue_position_id` parameter for `OrderStatusReport`
- Added bars update support for `Portfolio` PnLs (#2239), thanks @faysou
- Added optional `params` for `Strategy` order management methods (symmetry with `Actor` data methods) (#2251), thanks @faysou
- Added heartbeats for Betfair clients to keep streams alive (more robust when initial subscription delays)
- Added `timeout_shutdown` config option for `NautilusKernelConfig`
- Added IOC time in force mapping for Betfair orders
- Added `min_market_start_time` and `max_market_start_time` time range filtering for `BetfairInstrumentProviderConfig`
- Added `default_min_notional` config option for `BetfairInstrumentProviderConfig`
- Added `stream_conflate_ms` config option for `BetfairDataClientConfig`
- Added `recv_window_ms` config option for `BybitDataClientConfig` and `BybitExecClientConfig`
- Added `open_check_open_only` config option for `LiveExecEngineConfig`
- Added `BetSide` enum (to support `Bet` and `BetPosition`)
- Added `Bet` and `BetPosition` for betting market risk and PnL calculations
- Added `total_pnl` and `total_pnls` methods for `Portfolio`
- Added optional `price` parameter for `Portfolio` unrealized PnL and net exposure methods

### Breaking Changes
- Renamed `OptionsContract` instrument to `OptionContract` for more technically correct terminology (singular)
- Renamed `OptionsSpread` instrument to `OptionSpread` for more technically correct terminology (singular)
- Renamed `options_contract` modules to `option_contract` (see above)
- Renamed `options_spread` modules to `option_spread` (see above)
- Renamed `InstrumentClass.FUTURE_SPREAD` to `InstrumentClass.FUTURES_SPREAD` for more technically correct terminology
- Renamed `event_logging` config option to `log_events`
- Renamed `BetfairExecClientConfig.request_account_state_period` to `request_account_state_secs`
- Moved SQL schema directory to `schemas/sql` (reinstall the Nautilus CLI with `make install-cli`)
- Changed `OrderBookDelta` Arrow schema to use `FixedSizeBinary` fields to support the new precision modes
- Changed `OrderBookDepth10` Arrow schema to use `FixedSizeBinary` fields to support the new precision modes
- Changed `QuoteTick` Arrow schema to use `FixedSizeBinary` fields to support the new precision modes
- Changed `TradeTick` Arrow schema to use `FixedSizeBinary` fields to support the new precision modes
- Changed `Bar` Arrow schema to use `FixedSizeBinary` fields to support the new precision modes
- Changed `BettingInstrument` default `min_notional` to `None`
- Changed meaning of `ws_connection_delay_secs` for [PolymarketDataClientConfig](https://github.com/nautechsystems/nautilus_trader/blob/develop/nautilus_trader/adapters/polymarket/config.py) to be **non-initial** delay (#2271), thanks @ryantam626
- Changed `GATEIO` Tardis venue to `GATE_IO` for consistency with `CRYPTO_COM` and `BLOCKCHAIN_COM`
- Removed `max_ws_reconnection_tries` for dYdX configs (no longer applicable with infinite retries and exponential backoff)
- Removed `max_ws_reconnection_tries` for Bybit configs (no longer applicable with infinite retries and exponential backoff)
- Removed remaining `max_ws_reconnection_tries` for Bybit configs (#2290), thanks @sunlei

### Internal Improvements
- Added `ThrottledEnqueuer` for more efficient and robust live engines queue management and logging
- Added `OrderBookDeltaTestBuilder` in Rust to improve testing (#2234), thanks @filipmacek
- Added custom certificate loading for `SocketClient` TLS
- Added `check_nonempty_string` for string validation in Rust
- Improved Polymarket WebSocket subscription handling by configurable delay (#2271), thanks @ryantam626
- Improved `WebSocketClient` with state management, error handling, timeouts and robust reconnects with exponential backoff
- Improved `SocketClient` with state management, error handling, timeouts and robust reconnects with exponential backoff
- Improved `TradingNode` shutdown when running with `asyncio.run()` (more orderly handling of event loop)
- Improved `NautilusKernel` pending tasks cancellation on shutdown
- Improved `TardisHttpClient` requests and error handling
- Improved log file writer to strip ANSI escape codes and unprintable chars
- Improved `clean` make target behavior and added `distclean` make target (#2286), @demonkoryu
- Refined `Currency` `name` to accept non-ASCII characters (common for foreign currencies)
- Refactored CI with composite actions (#2242), thanks @sunlei
- Refactored Option Greeks feature (#2266), thanks @faysou
- Changed validation to allow zero commission for `PerContractFeeModel` (#2282), thanks @stefansimik
- Changed to use `mold` as the linker in CI (#2254), thanks @sunlei
- Ported market order processing for `OrderMatchingEngine` in Rust (#2202), thanks @filipmacek
- Ported limit order processing for `OrderMatchingEngine` in Rust (#2212), thanks @filipmacek
- Ported stop limit order processing for `OrderMatchingEngine` in Rust (#2225), thanks @filipmacek
- Ported `CancelOrder` processing for `OrderMatchingEngine` in Rust (#2231), thanks @filipmacek
- Ported `CancelAllOrders` processing for `OrderMatchingEngine` in Rust (#2253), thanks @filipmacek
- Ported `BatchCancelOrders` processing for `OrderMatchingEngine` in Rust (#2256), thanks @filipmacek
- Ported expire order processing for `OrderMatchingEngine` in Rust (#2259), thanks @filipmacek
- Ported modify order processing for `OrderMatchingEngine` in Rust (#2261), thanks @filipmacek
- Ported generate fresh account state for `SimulatedExchange` in Rust (#2272), thanks @filipmacek
- Ported adjust account for SimulatedExchange in Rust (#2273), thanks @filipmacek
- Continued porting `RiskEngine` to Rust (#2210), thanks @Pushkarm029
- Continued porting `ExecutionEngine` to Rust (#2214), thanks @Pushkarm029
- Continued porting `OrderEmulator` to Rust (#2219, #2226), thanks @Pushkarm029
- Moved `model` crate stubs into defaults (#2235), thanks @fhill2
- Upgraded `pyo3` crate to v0.23.4
- Upgraded `pyo3-async-runtimes` crate to v0.23.0

### Fixes
- Fixed `LiveTimer` immediate fire when start time zero (#2270), thanks for reporting @bartolootrit
- Fixed order book action parsing for Tardis (ensures zero sizes in snapshots work with the tighter validation for `action` vs `size`)
- Fixed PnL calculations for betting instruments in `Portfolio`
- Fixed net exposure for betting instruments in `Portfolio`
- Fixed backtest start and end time validation assertion (#2203), thanks @davidsblom
- Fixed `CustomData` import in `DataEngine` (#2207), thanks @graceyangfan and @faysou
- Fixed databento helper function (#2208), thanks @faysou
- Fixed live reconciliation of generated order fills to use the `venue_position_id` (when provided), thanks for reporting @sdk451
- Fixed `InstrumentProvider` initialization behavior when `reload` flag `True`, thanks @ryantam626
- Fixed handling of Binance HTTP error messages (not always JSON-parsable, leading to `msgspec.DecodeError`)
- Fixed `CARGO_TARGET_DIR` environment variable for build script (#2228), thanks @sunlei
- Fixed typo in `delta.rs` doc comment (#2230), thanks @eltociear
- Fixed memory leak in network PyO3 layer caused by the `gil-refs` feature (#2229), thanks for reporting @davidsblom
- Fixed reconnect handling for Betfair (#2232, #2288, #2289), thanks @limx0
- Fixed `instrument.id` null dereferences in error logs (#2237), thanks for reporting @ryantam626
- Fixed schema for listing markets of dYdX (#2240), thanks @davidsblom
- Fixed realized pnl calculation in `Portfolio` where flat positions were not included in cumulative sum (#2243), thanks @faysou
- Fixed update order in `Cache` for Rust (#2248), thanks @filipmacek
- Fixed websocket schema for market updates of dYdX (#2258), thanks @davidsblom
- Fixed handling of empty book messages for Tardis (resulted in `deltas` cannot be empty panicking)
- Fixed `Cache.bar_types` `aggregation_source` filtering, was incorrectly using `price_type` (#2269), thanks @faysou
- Fixed missing `combo` instrument type for Tardis integration
- Fixed quote tick processing from bars in `OrderMatchingEngine` resulting in sizes below the minimum increment (#2275), thanks for reporting @miller-moore
- Fixed initialization of `BinanceErrorCode`s requiring `int`
- Fixed resolution of Tardis `BINANCE_DELIVERY` venue for COIN-margined contracts
- Fixed hang in rate limiter (#2285), thanks @WyldeCat
- Fixed typo in `InstrumentProviderConfig` docstring (#2284), thanks @ikeepo
- Fixed handling of `tick_size_change` message for Polymarket

### Documentation Updates
- Added Databento overview tutorial (#2233, #2252), thanks @stefansimik
- Added docs for Actor (#2233), thanks @stefansimik
- Added docs for Portfolio limitations with bar data (#2233), thanks @stefansimik
- Added docs overview for example locations in repository (#2287), thanks @stefansimik
- Improved docstrings for Actor subscription and request methods
- Refined `streaming` parameter description (#2293), thanks @faysou and @stefansimik

### Deprecations
- The [talib](https://github.com/nautechsystems/nautilus_trader/tree/develop/nautilus_trader/indicators/ta_lib) subpackage for indicators is deprecated and will be removed in a future version, see [RFC](https://github.com/nautechsystems/nautilus_trader/issues/2206)

---

# NautilusTrader 1.210.0 Beta

Released on 10th January 2025 (UTC).

### Enhancements
- Added `PerContractFeeModel`, thanks @stefansimik
- Added `DYDXInternalError` and `DYDXOraclaPrice` data types for dYdX (#2155), thanks @davidsblom
- Added proper `OrderBookDeltas` flags parsing for Betfair
- Added Binance TradeLite message support (#2156), thanks @DeevsDeevs
- Added `DataEngineConfig.time_bars_skip_first_non_full_bar` config option (#2160), thanks @faysou
- Added `execution.fast` support for Bybit (#2165), thanks @sunlei
- Added catalog helper functions to export data (#2135), thanks @twitu
- Added additional timestamp properties for `NautilusKernel`
- Added `event_logging` config option for `StrategyConfig` (#2183), thanks @sunlei
- Added `bar_adaptive_high_low_ordering` to `BacktestVenueConfig` (#2188), thanks @faysou and @stefansimik

### Breaking Changes
- Removed optional `value` param from `UUID4` (use `UUID4.from_str(...)` instead), aligns with Nautilus PyO3 API
- Changed `unix_nanos_to_iso8601` to output an ISO 8601 (RFC 3339) format string with nanosecond precision
- Changed `format_iso8601` to output ISO 8601 (RFC 3339) format string with nanosecond precision
- Changed `format_iso8601` `dt` parameter to enforce `pd.Timestamp` (which has nanosecond precision)
- Changed `TradingNode.is_built` from a property to a method `.is_built()`
- Changed `TradingNode.is_running` from a property to a method `.is_running()`
- Changed `OrderInitialized` Arrow schema (`linked_order_ids` and `tags` data types changed from `string` to `binary`)
- Changed order dictionary representation field types for `avg_px` and `slippage`  from `str` to `float` (as out of alignment with position events)
- Changed `aggregation_source` filter parameter for `Cache.bar_types(...)` to optional with default of `None`

### Internal Improvements
- Improved market order handling when no size available in book (now explicitly rejects)
- Improved validation for `TradeTick` by ensuring `size` is always positive
- Improved validation for `OrderBookDelta` by ensuring `order.size` is positive when `action` is either `ADD` or `UPDATE`
- Improved validation for `BarSpecification` by ensuring `step` is always positive
- Standardized ISO 8601 timestamps to RFC 3339 spec with nanosecond precision
- Standardized flags for `OrderBookDeltas` parsing across adapters
- Refined parsing candles for dYdX (#2148), thanks @davidsblom
- Refined imports for type hints in Bybit (#2149), thanks @sunlei
- Refined private WebSocket message processing for Bybit (#2170), thanks @sunlei
- Refined WebSocket client re-subscribe log for Bybit (#2179), thanks @sunlei
- Refined margin balance report for dYdX (#2154), thanks @davidsblom
- Enhanced `lotSizeFilter` field for Bybit (#2166), thanks @sunlei
- Renamed WebSocket private client for Bybit (#2180), thanks @sunlei
- Added unit tests for custom dYdX types (#2163), thanks @davidsblom
- Allow bar aggregators to persist after `request_aggregated_bars` (#2144), thanks @faysou
- Handle directory and live streams to catalog (#2153), thanks @limx0
- Use timeout when initializing account for dYdX (#2169), thanks @davidsblom
- Use retry manager when sending websocket messages for dYdX (#2196), thanks @davidsblom
- Refined error logs when sending pong for dYdX (#2184), thanks @davidsblom
- Optimized message bus topic `is_matching` (#2151), thanks @ryantam626
- Added tests for `bar_adaptive_high_low_ordering` (#2197), thanks @faysou
- Ported `OrderManager` to Rust (#2161), thanks @Pushkarm029
- Ported trailing stop logic to Rust (#2174), thanks @DeevsDeevs
- Ported `FeeModel` to Rust (#2191), thanks @filipmacek
- Implemented IDs generator for `OrderMatchingEngine` in Rust (#2193), thanks @filipmacek
- Upgraded Cython to v3.1.0a1
- Upgraded `tokio` crate to v1.43.0
- Upgraded `datafusion` crate to v44.0.0

### Fixes
- Fixed type check for `DataClient` on requests to support clients other than `MarketDataClient`
- Fixed processing trade ticks from bars in `OrderMatchingEngine` - that could result in zero-size trades, thanks for reporting @stefansimik
- Fixed `instrument is None` check flows for `DataEngine` and `PolymarketExecutionClient`
- Fixed instrument updates in `BetfairDataClient` (#2152), thanks @limx0
- Fixed processing of time events on backtest completion when they occur after the final data timestamp
- Fixed missing enum member `CANCELED_MARKET_RESOLVED` for `PolymarketOrderStatus`
- Fixed missing `init_id` field from some order `.to_dict()` representations
- Fixed writing `DYDXOraclePrice` to catalog (#2158), thanks @davidsblom
- Fixed account balance for dYdX (#2167), thanks @davidsblom
- Fixed markets schema for dYdX (#2190), thanks @davidsblom
- Fixed missing `OrderEmulated` and `OrderReleased` Arrow schemas
- Fixed websocket public channel reconnect for Bybit (#2176), thanks @sunlei
- Fixed execution report parsing for Binance Spot (client order ID empty string now becomes a UUID4 string)
- Fixed docs typo for `fill_order` function in `OrderMatchingEngine` (#2189), thanks @filipmacek

### Documentation Updates
- Added docs for `Cache`, slippage and spread handling in backtesting (#2162), thanks @stefansimik
- Added docs for `FillModel` and bar based execution (#2187), thanks @stefansimik
- Added docs for choosing data (cost vs. accuracy) and bars OHLC processing (#2195), thanks @stefansimik
- Added docs for bar processing in backtests (#2198), thanks @stefansimik
- Added docs for timestamp and UUID specs

---

# NautilusTrader 1.209.0 Beta

Released on 25th December 2024 (UTC).

### Enhancements
- Added WebSocket API trading support for Bybit (#2129), thanks @sunlei
- Added `BybitOrderBookDeltaDataLoader` with tutorial for Bybit backtesting (#2131), thanks @DeevsDeevs
- Added margin and commission docs (#2128), thanks @stefansimik
- Added optional `depth` parameter for some `OrderBook` methods
- Added trade execution support where trades are processed by the matching engine (can be useful backtesting with throttled book and trades data)
- Refactored to use `exchange` MIC code as `venue` for instrument IDs with Databento GLBX dataset (#2108, #2121, #2124, #2126), thanks @faysou
- Refactored to use `self.config` attributes consistently (#2120), thanks @stefansimik

### Internal Improvements
- Optimized `UUID4::new()` avoiding unnecessary string allocation, achieving a ~2.8x performance improvement (added benches)
- Upgraded v4-proto for dYdX (#2136), thanks @davidsblom
- Upgraded `databento` crate to v0.17.0

### Breaking Changes
- Moved `BinanceOrderBookDeltaDataLoader` from `nautilus_trader.persistence.loaders` to `nautilus_trader.adapters.binance.loaders`

### Fixes
- Fixed multi-threaded monotonicity for `AtomicTime` in real-time mode
- Fixed timeout error code for Bybit (#2130), thanks @sunlei
- Fixed instruments info retrieval for Bybit (#2134), thanks @sunlei
- Fixed `request_aggregated_bars` metadata handling (#2137), thanks @faysou
- Fixed demo notebook `backtest_high_level.ipynb` (#2142), thanks @stefansimik

---

# NautilusTrader 1.208.0 Beta

Released on 15th December 2024 (UTC).

### Enhancements
- Added specific `params` for data subscriptions and requests which supports Databento `bbo-1s` and `bbo-1m` quotes (#2083, #2094), thanks @faysou
- Added support for `STOP_LIMIT` entry order type for `OrderFactory.bracket(...)`
- Added `.group_bids(...)` and `.group_asks(...)` for `OrderBook`
- Added `.bids_to_dict()` and `.asks_to_dict()` for `OrderBook`
- Added `ShutdownSystem` command and `shutdown_system(...)` method for components (system-wide shutdown for backtest, sandbox, or live environments)
- Added `max_ws_reconnection_tries` to `BybitDataClientConfig` (#2100), thanks @sunlei
- Added additional API functionality for Bybit (#2102), thanks @sunlei
- Added position and execution.fast subscriptions for Bybit (#2104), thanks @sunlei
- Added `max_ws_reconnection_tries` to `BybitExecClientConfig` (#2109), thanks @sunlei
- Added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee` params and attributes for `FuturesContract`
- Added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee` params and attributes for `FuturesSpread`
- Added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee` params and attributes for `OptionContract`
- Added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee` params and attributes for `OptionSpread`
- Improved Databento symbology support for Interactive Brokers (#2113), thanks @rsmb7z
- Improved support of `STOP_MARKET` and `STOP_LIMIT` orders for dYdX (#2069), thanks @Saransh-Bhandari
- Improved timer validation for `interval_ns` (avoids panicking from Rust)

### Internal Improvements
- Added `.bids_as_map()` and `.asks_as_map()` for `OrderBook` in Rust
- Added type stubs for `core` subpackage
- Added type stubs for `common` and `model` enums
- Added type stubs for `common.messages`
- Added re-exports and module declarations to enhance code ergonomics and improve import discoverability
- Added subscriptions for block height websocket messages for dYdX (#2085), thanks @davidsblom
- Added sccache in CI (#2093), thanks @sunlei
- Refined `BybitWebSocketClient` private channel authentication (#2101), thanks @sunlei
- Refined `BybitWebSocketClient` subscribe and unsubscribe (#2105), thanks @sunlei
- Refined place order class definitions for Bybit (#2106), thanks @sunlei
- Refined `BybitEnumParser` (#2107), thanks @sunlei
- Refined batch cancel orders for Bybit (#2111), thanks @sunlei
- Upgraded `tokio` crate to v1.42.0

### Breaking Changes
- Renamed `Level` to `BookLevel` (standardizes order book type naming conventions)
- Renamed `Ladder` to `BookLadder` (standardizes order book type naming conventions)
- Changed `FuturesContract` Arrow schema (added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee`)
- Changed `FuturesSpread` Arrow schema (added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee`)
- Changed `OptionContract` Arrow schema (added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee`)
- Changed `OptionSpread` Arrow schema (added `margin_init`, `margin_maint`, `maker_fee`, `taker_fee`)

### Fixes
- Fixed data requests when specifying `end` with no catalog registered (comparison between `pd.Timestamp` and `NoneType`)
- Fixed `BEST_EFFORT_CANCELED` order status report for dYdX (#2082), thanks @davidsblom
- Fixed order handling for `BEST_EFFORT_CANCELED` messages of dYdX (#2095), thanks @davidsblom
- Fixed specifying price for market orders on dYdX (#2088), thanks @davidsblom
- Fixed interest rate curve custom data and interpolation (#2090), thanks @gcheshkov
- Fixed `BybitHttpClient` error handling when not a JSON string (#2096), thanks @sunlei
- Fixed `BybitWebSocketClient` private channel reconnect (#2097), thanks @sunlei
- Fixed incorrect order side use in `BybitExecutionClient` (#2098), thanks @sunlei
- Fixed default `http_base_url` for Bybit (#2110), thanks @sunlei

---

# NautilusTrader 1.207.0 Beta

Released on 29th November 2024 (UTC).

### Enhancements
- Implemented mixed catalog data requests with catalog update (#2043), thanks @faysou
- Added Databento symbology support for Interactive Brokers (#2073), thanks @rsmb7z
- Added `metadata` parameter for data requests (#2043), thanks @faysou
- Added `STOP_MARKET` and `STOP_LIMIT` order support for dYdX (#2066), thanks @davidsblom
- Added `max_reconnection_tries` to data client config for dYdX (#2066), thanks @davidsblom
- Added wallet subscription for Bybit (#2076), thanks @sunlei
- Added docs clarity on loading historical bars (#2078), thanks @dodofarm
- Added `price_precision` optional parameter for `DatabentoDataLoader` methods
- Improved `Cache` behavior when adding more recent quotes, trades, or bars (now adds to cache)

### Internal Improvements
- Ported `Portfolio` and `AccountManager` to Rust (#2058), thanks @Pushkarm029
- Implemented `AsRef<str>` for `Price`, `Money`, and `Currency`
- Improved expired timer cleanup in clocks (#2064), thanks @twitu
- Improved live engines error logging (will now log all exceptions rather than just `RuntimeError`)
- Improved symbol normalization for Tardis
- Improved historical bar request performance for Tardis
- Improved `TradeId` Debug implementation to display value as proper UTF-8 string
- Refined `HttpClient` for use directly from Rust
- Refined Databento decoder (removed currency hard coding and use of `unsafe`)
- Upgraded `datafusion` crate to v43.0.0 (#2056), thanks @twitu

### Breaking Changes
- Renamed `TriggerType.LAST_TRADE` to `LAST_PRICE` (more conventional terminology)

### Fixes
- Fixed missing venue -> exchange mappings for Tardis integration
- Fixed account balance and order status parsing for dYdX (#2067), thanks @davidsblom
- Fixed parsing best effort opened order status for dYdX (#2068), thanks @davidsblom
- Fixed occasionally incorrect `price_precision`, `multiplier` and `lot_size` decoding for Databento instruments
- Fixed missing Arrow schemas for instrument deserialization
- Reconcile order book for dYdX when inconsistent (#2077), thanks @davidsblom

---

# NautilusTrader 1.206.0 Beta

Released on 17th November 2024 (UTC).

### Enhancements
- Added `TardisDataClient` providing live data streams from a Tardis Machine WebSocket server
- Added `TardisInstrumentProvider` providing instrument definitions from Tardis through the HTTP instrument metadata API
- Added `Portfolio.realized_pnl(...)` method for per instrument realized PnL (based on positions)
- Added `Portfolio.realized_pnls(...)` method for per venue realized PnL (based on positions)
- Added configuration warning for `InstrumentProvider` (to warn when node starts with no instrument loading)
- Implemented Tardis optional [symbol normalization](https://nautilustrader.io/docs/nightly/integrations/tardis/#symbology-and-normalization)
- Implemented `WebSocketClient` reconnection retries (#2044), thanks @davidsblom
- Implemented `OrderCancelRejected` event generation for Binance and Bybit
- Implemented `OrderModifyRejected` event generation for Binance and Bybit
- Improved `OrderRejected` handling of `reason` string (`None` is now allowed which will become the string `'None'`)
- Improved `OrderCancelRejected` handling of `reason` string (`None` is now allowed which will become the string `'None'`)
- Improved `OrderModifyRejected` handling of `reason` string (`None` is now allowed which will become the string `'None'`)

### Internal Improvements
- Ported `RiskEngine` to Rust (#2035), thanks @Pushkarm029 and @twitu
- Ported `ExecutionEngine` to Rust (#2048), thanks @twitu
- Added globally shared data channels to send events from engines to Runner in Rust (#2042), thanks @twitu
- Added LRU caching for dYdX HTTP client (#2049), thanks @davidsblom
- Improved identifier constructors to take `AsRef<str>` for a cleaner more flexible API
- Refined identifiers `From` trait impls
- Refined `InstrumentProvider` initialization behavior and logging
- Refined `LiveTimer` cancel and performance testing
- Simplified `LiveTimer` cancellation model (#2046), thanks @twitu
- Refined Bybit HMAC authentication signatures (now using Rust implemented function)
- Refined Tardis instrument ID parsing
- Removed Bybit `msgspec` redundant import alias (#2050), thanks @sunlei
- Upgraded `databento` crate to v0.16.0

### Breaking Changes
None

### Fixes
- Fixed loading specific instrument IDs for `InstrumentProviderConfig`
- Fixed PyO3 instrument conversions for `raw_symbol` (was incorrectly using the normalized symbol)
- Fixed reconcile open orders and account websocket message for dYdX (#2039), thanks @davidsblom
- Fixed market order `avg_px` for Polymarket trade reports
- Fixed Betfair clients keepalive (#2040), thanks @limx0
- Fixed Betfair reconciliation (#2041), thanks @limx0
- Fixed Betfair customer order ref limit to 32 chars
- Fixed Bybit handling of `PARTIALLY_FILLED_CANCELED` status orders
- Fixed Polymarket size precision for `BinaryOption` instruments (precision 6 to match USDC.e)
- Fixed adapter instrument reloading (providers were not reloading instruments at the configured interval due to internal state flags)
- Fixed static time logging for `BacktestEngine` when running with `use_pyo3` logging config
- Fixed in-flight orders check and improve error handling (#2053), thanks @davidsblom
- Fixed dYdX handling for liquidated fills (#2052), thanks @davidsblom
- Fixed `BybitResponse.time` field as optional `int` (#2051), thanks @sunlei
- Fixed single instrument requests for `DatabentoDataClient` (was incorrectly calling `_handle_instruments` instead of `_handle_instrument`), thanks for reporting @Emsu
- Fixed `fsspec` recursive globbing behavior to ensure only file paths are included, and bumped dependency to version 2024.10.0
- Fixed jupyterlab url typo (#2057), thanks @Alsheh

---

# NautilusTrader 1.205.0 Beta

Released on 3rd November 2024 (UTC).

### Enhancements
- Added Tardis Machine and HTTP API integration in Python and Rust
- Added `LiveExecEngineConfig.open_check_interval_secs` config option to actively reconcile open orders with the venue
- Added aggregation of bars from historical data (#2002), thanks @faysou
- Added monthly and weekly bar aggregations (#2025), thanks @faysou
- Added `raise_exception` optional parameter to `TradingNode.run` (#2021), thanks @faysou
- Added `OrderBook.get_avg_px_qty_for_exposure` in Rust (#1893), thanks @elementace
- Added timeouts to Interactive Brokers adapter configurations (#2026), thanks @rsmb7z
- Added optional time origins for time bar aggregation (#2028), thanks @faysou
- Added Polymarket position status reports and order status report generation based on fill reports
- Added USDC.e (PoS) currency (used by Polymarket) to internal currency map
- Upgraded Polymarket WebSocket API to new version

### Internal Improvements
- Ported analysis subpackage to Rust (#2016), thanks @Pushkarm029
- Improved Postgres testing (#2018), thanks @filipmacek
- Improved Redis version parsing to support truncated versions (improves compatibility with Redis-compliant databases)
- Refined Arrow serialization (record batch functions now also available in Rust)
- Refined core `Bar` API to remove unnecessary unwraps
- Standardized network client logging
- Fixed all PyO3 deprecations for API breaking changes
- Fixed all clippy warning lints for PyO3 changes (#2030), thanks @Pushkarm029
- PyO3 upgrade refactor and repair catalog tests (#2032), thanks @twitu
- Upgraded `pyo3` crate to v0.22.5
- Upgraded `pyo3-async-runtimes` crate to v0.22.0
- Upgraded `tokio` crate to v1.41.0

### Breaking Changes
- Removed PyO3 `DataTransformer` (was being used for namespacing, so refactored to separate functions)
- Moved `TEST_DATA_DIR` constant from `tests` to `nautilus_trader` package (#2020), thanks @faysou

### Fixes
- Fixed use of Redis `KEYS` command which, is unsupported in cluster environments (replaced with `SCAN` for compatibility)
- Fixed decoding fill HTTP messages for dYdX (#2022), thanks @davidsblom
- Fixed account balance report for dYdX (#2024), thanks @davidsblom
- Fixed Interactive Brokers market data client subscription log message (#2012), thanks @marcodambros
- Fixed Polymarket execution reconciliation (was not able to reconcile from closed orders)
- Fixed catalog query mem leak test (#2031), thanks @Pushkarm029
- Fixed `OrderInitialized.to_dict()` `tags` value type to `list[str]` (was a concatenated `str`)
- Fixed `OrderInitialized.to_dict()` `linked_order_ids` value type to `list[str]` (was a concatenated `str`)
- Fixed Betfair clients shutdown (#2037), thanks @limx0

---

# NautilusTrader 1.204.0 Beta

Released on 22nd October 2024 (UTC).

### Enhancements
- Added `TardisCSVDataLoader` for loading data from Tardis format CSV files as either legacy Cython or PyO3 objects
- Added `Clock.timestamp_us()` method for UNIX timestamps in microseconds (μs)
- Added support for `bbo-1s` and `bbo-1m` quote schemas for Databento adapter (#1990), thanks @faysou
- Added validation for venue `book_type` configuration vs data (prevents an issue where top-of-book data is used when order book data is expected)
- Added `compute_effective_deltas` config option for `PolymarketDataClientConfig`, reducing snapshot size (default `False` to retain current behavior)
- Added rate limiter for `WebSocketClient` (#1994), thanks @Pushkarm029
- Added in the money probability field to GreeksData (#1995), thanks @faysou
- Added `on_signal(signal)` handler for custom signal data
- Added `nautilus_trader.common.events` module with re-exports for `TimeEvent` and other system events
- Improved usability of `OrderBookDepth10` by filling partial levels with null orders and zero counts
- Improved Postgres config (#2010), thanks @filipmacek
- Refined `DatabentoInstrumentProvider` handling of large bulks of instrument definitions (improved parent symbol support)
- Standardized Betfair symbology to use hyphens instead of periods (prevents Betfair symbols being treated as composite)
- Integration guide docs fixes (#1991), thanks @FarukhS52

### Internal Improvements
- Ported `Throttler` to Rust (#1988), thanks @Pushkarm029 and @twitu
- Ported `BettingInstrument` to Rust
- Refined `RateLimiter` for `WebSocketClient` and add tests (#2000), thanks @Pushkarm029
- Refined `WebSocketClient` to close existing tasks on reconnect (#1986), thanks @davidsblom
- Removed mutable references in `CacheDatabaseAdapter` trait in Rust (#2015), thanks @filipmacek
- Use Rust rate limiter for dYdX websockets (#1996, #1999), thanks @davidsblom
- Improved error logs for dYdX websocket subscriptions (#1993), thanks @davidsblom
- Standardized log and error message syntax in Rust
- Continue porting `SimulatedExchange` and `OrderMatchingEngine` to Rust (#1997, #1998, #2001, #2003, #2004, #2006, #2007, #2009, #2014), thanks @filipmacek

### Breaking Changes
- Removed legacy `TardisQuoteDataLoader` (now redundant with new Rust implemented loader)
- Removed legacy `TardisTradeDataLoader` (now redundant with new Rust implemented loader)
- Custom signals are now passed to `on_signal(signal)` instead of `on_data(data)`
- Changed `Position.to_dict()` `commissions` value type to `list[str]` (was an optional `str` of a list of strings)
- Changed `Position.to_dict()` `avg_px_open` value type to `float`
- Changed `Position.to_dict()` `avg_px_close` value type to `float | None`
- Changed `Position.to_dict()` `realized_return` value type to `float | None`
- Changed `BettingInstrument` Arrow schema fields `event_open_date` and `market_start_time` from `string` to `uint64`

### Fixes
- Fixed `SocketClient` TLS implementation
- Fixed `WebSocketClient` error handling on writer close, thanks for reporting @davidsblom
- Fixed resubscribing to orderbook in batched mode for dYdX (#1985), thanks @davidsblom
- Fixed Betfair tests related to symbology (#1988), thanks @limx0
- Fixed check for `OmsType` in `OrderMatchingEngine` position ID processing (#2003), thanks @filipmacek
- Fixed `TardisCSVDataLoader` snapshot5 and snapshot25 parsing (#2005), thanks @Pushkarm029
- Fixed Binance clients venue assignment, we should use the `client_id` params (which match the custom client `name`) to communicate with the clients, and use the same `'BINANCE'` venue identifiers
- Fixed `OrderMatchingEngine` incorrectly attempting to process monthly bars for execution (which will fail, as no reasonable `timedelta` is available), thanks for reporting @frostRed
- Fixed handling `MONTH` aggregation for `cache.bar_types()` (sorting required an internal call for the bar intervals `timedelta`), thanks for reporting @frostRed

---

# NautilusTrader 1.203.0 Beta

Released on 5th October 2024 (UTC).

### Enhancements
- Added `mode` parameter to `ParquetDataCatalog.write_data` to control data writing behavior (#1976), thanks @faysou
- Added batch cancel for short terms orders of dYdX (#1978), thanks @davidsblom
- Improved OKX configuration (#1966), thanks @miller-moore
- Improved option greeks (#1964), thanks @faysou

### Internal Improvements
- Implemented order book delta processing for `SimulatedExchange` in Rust (#1975), thanks @filipmacek
- Implemented bar processing for `SimulatedExchange` in Rust (#1969), thanks @filipmacek
- Implemented remaining getter functions for `SimulatedExchange` in Rust (#1970), thanks @filipmacek
- Implemented rate limiting for dYdX websocket subscriptions (#1977), thanks @davidsblom
- Refactored reconnection handling for dYdX (#1983), thanks @davidsblom
- Refined `DatabentoDataLoader` internals to accommodate usage from Rust
- Added initial large test data files download and caching capability

### Breaking Changes
None

### Fixes
- Fixed out of order row groups in DataFusion filter query (#1974), thanks @twitu
- Fixed `BacktestNode` data sorting regression causing clock non-decreasing time assertion error
- Fixed circular imports for `Actor`, thanks @limx0
- Fixed OKX HTTP client signatures (#1966), thanks @miller-moore
- Fixed resubscribing to orderbooks for dYdX (#1973), thanks @davidsblom
- Fixed generating cancel rejections for dYdX (#1982), thanks @davidsblom
- Fixed `WebSocketClient` task cleanup on disconnect (#1981), thanks @twitu
- Fixed `Condition` method name collisions with C `true` and `false` macros, which occurred during compilation in profiling mode

---

# NautilusTrader 1.202.0 Beta

Released on 27th September 2024 (UTC).

This will be the final release with support for Python 3.10.

The `numpy` version requirement has been relaxed to >= 1.26.4.

### Enhancements
- Added Polymarket decentralized prediction market integration
- Added OKX crypto exchange integration (#1951), thanks @miller-moore
- Added `BinaryOption` instrument (supports Polymarket integration)
- Added `LiveExecutionEngine.inflight_check_retries` config option to limit in-flight order query attempts
- Added `Symbol.root()` method for obtaining the root of parent or composite symbols
- Added `Symbol.topic()` method for obtaining the subscription topic of parent or composite symbols
- Added `Symbol.is_composite()` method to determine if symbol is made up of parts with period (`.`) delimiters
- Added `underlying` filter parameter for `Cache.instruments(...)` method
- Added `reduce_only` parameter for `Strategy.close_position(...)` method (default `True` to retain current behavior)
- Added `reduce_only` parameter for `Strategy.close_all_positions(...)` method (default `True` to retain current behavior)
- Implemented flush with truncate Postgres function for `PostgresCacheDatabase` (#1928), thanks @filipmacek
- Implemented file rotation for `StreamingFeatherWriter` with internal improvements using `Clock` and `Cache` (#1954, #1961), thanks @graceyangfan
- Improved dYdX execution client to use `RetryManager` for HTTP requests (#1941), thanks @davidsblom
- Improved Interactive Brokers adapter to use a dynamic IB gateway `container_image` from config (#1940), thanks @rsmb7z
- Improved `OrderBookDeltas` streaming and batching based on the `F_LAST` flag
- Standardized underscore thousands separators for backtest logging
- Updated Databento `publishers.json`

### Internal Improvements
- Implemented `OrderTestBuilder` to assist testing in Rust (#1952), thanks @filipmacek
- Implemented quote tick processing for SimulatedExchange in Rust (#1956), thanks @filipmacek
- Implemented trade tick processing for SimulatedExchange in Rust (#1956), thanks @filipmacek
- Refined `Logger` to use unbuffered stdout/stderr writers (#1960), thanks @twitu

### Breaking Changes
- Renamed `batch_size_bytes` to `chunk_size` (more accurate naming for number of data points to process per chunk in backtest streaming mode)
- Standardized Stop-Loss (SL) and Take-Profit (TP) parameter ordering for `OrderFactory.bracket(...)` including: `tp_time_in_force`, `tp_exec_algorithm_params`, `tp_tags`, `tp_client_order_id`

### Fixes
- Fixed `LoggingConfig` issue for `level_file` when used with `use_pyo3=True` (was not passing through the `level_file` setting), thanks for reporting @xt2014
- Fixed composite bar requests (#1923), thanks @faysou
- Fixed average price calculation for `ValueBarAggregator` (#1927), thanks @faysou
- Fixed breaking protobuf issue by pinning `protobuf` and `grpcio` for dYdX (#1929), thanks @davidsblom
- Fixed edge case where exceptions raised in `BacktestNode` prior to engine initialization would not produce logs, thanks for reporting @faysou
- Fixed handling of internal server error for dYdX (#1938), thanks @davidsblom
- Fixed `BybitWebSocketClient` private channel authentication on reconnect, thanks for reporting @miller-moore
- Fixed `OrderFactory.bracket(...)` parameter ordering for `sl_time_in_force` and `tp_time_in_force`, thanks for reporting @marcodambros
- Fixed `Cfd` instrument Arrow schema and serialization
- Fixed bar subscriptions on TWS/GW restart for Interactive Brokers (#1950), thanks @rsmb7z
- Fixed Databento parent and continuous contract subscriptions (using new symbol root)
- Fixed Databento `FuturesSpread` and `OptionSpread` instrument decoding (was not correctly handling price increments and empty underlyings)
- Fixed `FuturesSpread` serialization
- Fixed `OptionSpread` serialization

---

# NautilusTrader 1.201.0 Beta

Released on 9th September 2024 (UTC).

### Enhancements
- Added order book deltas triggering support for `OrderEmulator`
- Added `OrderCancelRejected` event generation for dYdX adapter (#1916), thanks @davidsblom
- Refined handling of Binance private key types (RSA, Ed25519) and integrated into configs
- Implemented cryptographic signing in Rust (replacing `pycryptodome` for Binance)
- Removed the vendored `tokio-tungstenite` crate (#1902), thanks @VioletSakura-7

### Breaking Changes
None

### Fixes
- Fixed `BinanceFuturesEventType` by adding new `TRADE_LITE` member, reflecting the Binance update on 2024-09-03 (UTC)

---

# NautilusTrader 1.200.0 Beta

Released on 7th September 2024 (UTC).

### Enhancements
- Added dYdX integration (#1861, #1868, #1873, #1874, #1875, #1877, #1879, #1880, #1882, #1886, #1887, #1890, #1891, #1896, #1901, #1903, #1907, #1910, #1911, #1913, #1915), thanks @davidsblom
- Added composite bar types, bars aggregated from other bar types (#1859, #1885, #1888, #1894, #1905), thanks @faysou
- Added `OrderBookDeltas.batch` for batching groups of deltas based on record flags (batch until `F_LAST`)
- Added `OrderBookDeltas` batching support for `ParquetDataCatalog` (use `data_cls` of `OrderBookDeltas` to batch with the same flags method as live adapters)
- Added `RetryManagerPool` to abstract common retry functionality for all adapters
- Added `InstrumentClose` functionality for `OrderMatchingEngine`, thanks @limx0
- Added `BacktestRunConfig.dispose_on_completion` config option to control post-run disposal behavior for each internal backtest engine (default `True` to retain current behavior)
- Added `recv_window_ms` config option for `BinanceExecClientConfig`
- Added `sl_time_in_force` and `tp_time_in_force` parameters to `OrderFactory.bracket(...)` method
- Added custom `client_order_id` parameters to `OrderFactory` methods
- Added support for Binance RSA and Ed25519 API key types (#1908), thanks @NextThread
- Added `multiplier` parameter for `CryptoPerpetual` (default 1)
- Implemented `BybitExecutionClient` retry logic for `submit_order`, `modify_order`, `cancel_order` and `cancel_all_orders`
- Improved error modeling and handling in Rust (#1866), thanks @twitu
- Improved `HttpClient` error handling and added `HttpClientError` exception for Python (#1872), thanks @twitu
- Improved `WebSocketClient` error handling and added `WebSocketClientError` exception for Python (#1876), thanks @twitu
- Improved `WebSocketClient.send_text` efficiency (now accepts UTF-8 encoded bytes, rather than a Python string)
- Improved `@customdataclass` decorator with `date` field and refined `__repr__` (#1900, #1906, #1909), thanks @faysou
- Improved standardization of `OrderBookDeltas` parsing and records flags for crypto venues
- Refactored `RedisMessageBusDatabase` to tokio tasks
- Refactored `RedisCacheDatabase` to tokio tasks
- Upgraded `tokio` crate to v1.40.0

### Breaking Changes
- Renamed `heartbeat_interval` to `heartbeat_interval_secs` (more explicitly indicates time units)
- Moved `heartbeat_interval_secs` config option to `MessageBusConfig` (the message bus handles external stream processing)
- Changed `WebSocketClient.send_text(...)` to take `data` as `bytes` rather than `str`
- Changed `CryptoPerpetual` Arrow schema to include `multiplier` field
- Changed `CryptoFuture` Arrow schema to include `multiplier` field

### Fixes
- Fixed `OrderBook` memory deallocation in Python finalizer (memory was not being freed on object destruction), thanks for reporting @zeyuhuan
- Fixed `Order` tags serialization (was not concatenating to a single string), thanks for reporting @DevRoss
- Fixed `types_filter` serialization in `MessageBusConfig` during kernel setup
- Fixed `InstrumentProvider` handling of `load_ids_on_start` when elements are already `InstrumentId`s
- Fixed `InstrumentProviderConfig` hashing for `filters` field

---

# NautilusTrader 1.199.0 Beta

Released on 19th August 2024 (UTC).

### Enhancements
- Added `LiveExecEngineConfig.generate_missing_orders` reconciliation config option to align internal and external position states
- Added `LogLevel::TRACE` (only available in Rust for debug/development builds)
- Added `Actor.subscribe_signal(...)` method and `Data.is_signal(...)` class method (#1853), thanks @faysou
- Added Binance Futures support for `HEDGE` mode (#1846), thanks @DevRoss
- Overhauled and refined error modeling and handling in Rust (#1849, #1858), thanks @twitu
- Improved `BinanceExecutionClient` position report requests (can now filter by instrument and includes reporting for flat positions)
- Improved `BybitExecutionClient` position report requests (can now filter by instrument and includes reporting for flat positions)
- Improved `LiveExecutionEngine` reconciliation robustness and recovery when internal positions do not match external positions
- Improved `@customdataclass` decorator constructor to allow more positional arguments (#1850), thanks @faysou
- Improved `@customdataclass` documentation (#1854), thanks @faysou
- Upgraded `datafusion` crate to v41.0.0
- Upgraded `tokio` crate to v1.39.3
- Upgraded `uvloop` to v0.20.0 (upgrades libuv to v1.48.0)

### Breaking Changes
- Changed `VolumeWeightedAveragePrice` calculation formula to use each bars "typical" price (#1842), thanks @evgenii-prusov
- Changed `OptionContract` constructor parameter ordering and Arrow schema (consistently group option kind and strike price)
- Renamed `snapshot_positions_interval` to `snapshot_positions_interval_secs` (more explicitly indicates time units)
- Moved `snapshot_orders` config option to `ExecEngineConfig` (can now be used for all environment contexts)
- Moved `snapshot_positions` config option to `ExecEngineConfig` (can now be used for all environment contexts)
- Moved `snapshot_positions_interval_secs` config option to `ExecEngineConfig` (can now be used for all environment contexts)

### Fixes
- Fixed `Position` exception type on duplicate fill (should be `KeyError` to align with the same error for `Order`)
- Fixed Bybit position report parsing when position is flat (`BybitPositionSide` now correctly handles the empty string)

---

# NautilusTrader 1.198.0 Beta

Released on 9th August 2024 (UTC).

### Enhancements
- Added `@customdataclass` decorator to reduce need for boiler plate implementing custom data types (#1828), thanks @faysou
- Added timeout for HTTP client in Rust (#1835), thanks @davidsblom
- Added catalog conversion function of streamed data to backtest data (#1834), thanks @faysou
- Upgraded Cython to v3.0.11

### Breaking Changes
None

### Fixes
- Fixed creation of `instrument_id` folder when writing PyO3 bars in catalog (#1832), thanks @faysou
- Fixed `StreamingFeatherWriter` handling of `include_types` option (#1833), thanks @faysou
- Fixed `BybitExecutionClient` position reports error handling and logging
- Fixed `BybitExecutionClient` order report handling to correctly process external orders

---

# NautilusTrader 1.197.0 Beta

Released on 2nd August 2024 (UTC).

### Enhancements
- Added Databento Status schema support for loading and live trading
- Added options on futures support for Interactive Brokers (#1795), thanks @rsmb7z
- Added documentation for option greeks custom data example (#1788), thanks @faysou
- Added `MarketStatusAction` enum (support Databento `status` schema)
- Added `ignore_quote_tick_size_updates` config option for Interactive Brokers (#1799), thanks @sunlei
- Implemented `MessageBus` v2 in Rust (#1786), thanks @twitu
- Implemented `DataEngine` v2 in Rust (#1785), thanks @twitu
- Implemented `FillModel` in Rust (#1801), thanks @filipmacek
- Implemented `FixedFeeModel` in Rust (#1802), thanks @filipmacek
- Implemented `MakerTakerFeeModel` in Rust (#1803), thanks @filipmacek
- Implemented Postgres native enum mappings in Rust (#1797, #1806), thanks @filipmacek
- Refactored order submission error handling for Interactive Brokers (#1783), thanks @rsmb7z
- Improved live reconciliation robustness (will now generate inferred orders necessary to align external position state)
- Improved tests for Interactive Brokers (#1776), thanks @mylesgamez
- Upgraded `tokio` crate to v1.39.2
- Upgraded `datafusion` crate to v40.0.0

### Breaking Changes
- Removed `VenueStatus` and all associated methods and schemas (redundant with `InstrumentStatus`)
- Renamed `QuoteTick.extract_volume(...)` to `.extract_size(...)` (more accurate terminology)
- Changed `InstrumentStatus` params (support Databento `status` schema)
- Changed `InstrumentStatus` Arrow schema
- Changed `OrderBook` FFI API to take data by reference instead of by value

### Fixes
- Fixed rounding errors in accounting calculations for large values (using `decimal.Decimal` internally)
- Fixed multi-currency account commission handling with multiple PnL currencies (#1805), thanks for reporting @dpmabo
- Fixed `DataEngine` unsubscribing from order book deltas (#1814), thanks @davidsblom
- Fixed `LiveExecutionEngine` handling of adapter client execution report causing `None` mass status (#1789), thanks for reporting @faysou
- Fixed `InteractiveBrokersExecutionClient` handling of instruments not found when generating execution reports (#1789), thanks for reporting @faysou
- Fixed Bybit parsing of trade and quotes for websocket messages (#1794), thanks @davidsblom

---

# NautilusTrader 1.196.0 Beta

Released on 5th July 2024 (UTC).

### Enhancements
- Added `request_order_book_snapshot` method (#1745), thanks @graceyangfan
- Added order book data validation for `BacktestNode` when a venue `book_type` is `L2_MBP` or `L3_MBO`
- Added Bybit demo account support (set `is_demo` to `True` in configs)
- Added Bybit stop order types (`STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`, `LIMIT_IF_TOUCHED`, `TRAILING_STOP_MARKET`)
- Added Binance venue option for adapter configurations (#1738), thanks @DevRoss
- Added Betfair amend order quantity support (#1687 and #1751), thanks @imemo88 and @limx0
- Added Postgres tests serial test group for nextest runner (#1753), thanks @filipmacek
- Added Postgres account persistence capability (#1768), thanks @filipmacek
- Refactored `AccountAny` pattern in Rust (#1755), thanks @filipmacek
- Changed `DatabentoLiveClient` to use new [snapshot on subscribe](https://databento.com/blog/live-MBO-snapshot) feature
- Changed identifier generator time tag component to include seconds (affects new `ClientOrderId`, `OrderId` and `PositionId` generation)
- Changed `<Arc<Mutex<bool>>` to `AtomicBool` in Rust `network` crate, thanks @NextThread and @twitu
- Ported `KlingerVolumeOscillator` indicator to Rust (#1724), thanks @Pushkarm029
- Ported `DirectionalMovement` indicator to Rust (#1725), thanks @Pushkarm029
- Ported `ArcherMovingAveragesTrends` indicator to Rust (#1726), thanks @Pushkarm029
- Ported `Swings` indicator to Rust (#1731), thanks @Pushkarm029
- Ported `BollingerBands` indicator to Rust (#1734), thanks @Pushkarm029
- Ported `VolatilityRatio` indicator to Rust (#1735), thanks @Pushkarm029
- Ported `Stochastics` indicator to Rust (#1736), thanks @Pushkarm029
- Ported `Pressure` indicator to Rust (#1739), thanks @Pushkarm029
- Ported `PsychologicalLine` indicator to Rust (#1740), thanks @Pushkarm029
- Ported `CommodityChannelIndex` indicator to Rust (#1742), thanks @Pushkarm029
- Ported `LinearRegression` indicator to Rust (#1743), thanks @Pushkarm029
- Ported `DonchianChannel` indicator to Rust (#1744), thanks @Pushkarm029
- Ported `KeltnerChannel` indicator to Rust (#1746), thanks @Pushkarm029
- Ported `RelativeVolatilityIndex` indicator to Rust (#1748), thanks @Pushkarm029
- Ported `RateOfChange` indicator to Rust (#1750), thanks @Pushkarm029
- Ported `MovingAverageConvergenceDivergence` indicator to Rust (#1752), thanks @Pushkarm029
- Ported `OnBalanceVolume` indicator to Rust (#1756), thanks @Pushkarm029
- Ported `SpreadAnalyzer` indicator to Rust (#1762), thanks @Pushkarm029
- Ported `KeltnerPosition` indicator to Rust (#1763), thanks @Pushkarm029
- Ported `FuzzyCandlesticks` indicator to Rust (#1766), thanks @Pushkarm029

### Breaking Changes
- Renamed `Actor.subscribe_order_book_snapshots` and `unsubscribe_order_book_snapshots` to `subscribe_order_book_at_interval` and `unsubscribe_order_book_at_interval` respectively (this clarifies the method behavior where the handler then receives `OrderBook` at a regular interval, distinct from a collection of deltas representing a snapshot)

### Fixes
- Fixed `LIMIT` order fill behavior for `L2_MBP` and `L3_MBO` book types (was not honoring limit price as maker), thanks for reporting @dpmabo
- Fixed `CashAccount` PnL calculations when opening a position with multiple fills, thanks @Otlk
- Fixed msgspec encoding and decoding of `Environment` enum for `NautilusKernelConfig`
- Fixed `OrderMatchingEngine` processing by book type for quotes and deltas (#1754), thanks @davidsblom
- Fixed `DatabentoDataLoader.from_dbn_file` for `OrderBookDelta`s when `as_legacy_cython=False`
- Fixed `DatabentoDataLoader` OHLCV bar schema loading (incorrectly accounting for display factor), thanks for reporting @faysou
- Fixed `DatabentoDataLoader` multiplier and round lot size decoding, thanks for reporting @faysou
- Fixed Binance order report generation `active_symbols` type miss matching (#1729), thanks @DevRoss
- Fixed Binance trade data websocket schemas (Binance no longer publish `b` buyer and `a` seller order IDs)
- Fixed `BinanceFuturesInstrumentProvider` parsing of min notional, thanks for reporting @AnthonyVince
- Fixed `BinanceSpotInstrumentProvider` parsing of min and max notional
- Fixed Bybit order book deltas subscriptions for `INVERSE` product type
- Fixed `Cache` documentation for `get` (was the same as `add`), thanks for reporting @faysou

---

# NautilusTrader 1.195.0 Beta

Released on 17th June 2024 (UTC).

### Enhancements
- Added Bybit base coin for fee rate parsing (#1696), thanks @filipmacek
- Added `IndexInstrument` with support for Interactive Brokers (#1703), thanks @rsmb7z
- Refactored Interactive Brokers client and gateway configuration (#1692), thanks @rsmb7z
- Improved `InteractiveBrokersInstrumentProvider` contract loading (#1699), thanks @rsmb7z
- Improved `InteractiveBrokersInstrumentProvider` option chain loading (#1704), thanks @rsmb7z
- Improved `Instrument.make_qty` error clarity when a positive value is rounded to zero
- Updated installation from source docs for Clang dependency (#1690), thanks @Troubladore
- Updated `DockerizedIBGatewayConfig` docs (#1691), thanks @Troubladore

### Breaking Changes
None

### Fixes
- Fixed DataFusion streaming backend mem usage (now constant mem usage) (#1693), thanks @twitu
- Fixed `OrderBookDeltaDataWrangler` snapshot parsing (was not prepending a `CLEAR` action), thanks for reporting @VeraLyu
- Fixed `Instrument.make_price` and `make_qty` when increments have a lower precision (was not rounding to the minimum increment)
- Fixed `EMACrossTrailingStop` example strategy trailing stop logic (could submit multiple trailing stops on partial fills)
- Fixed Binance `TRAILING_STOP_MARKET` orders (callback rounding was incorrect, was also not handling updates)
- Fixed Interactive Brokers multiple gateway clients (incorrect port handling in factory) (#1702), thanks @dodofarm
- Fixed time alerts Python example in docs (#1713), thanks @davidsblom

---

# NautilusTrader 1.194.0 Beta

Released on 31st May 2024 (UTC).

### Enhancements
- Added `DataEngine` order book deltas buffering to `F_LAST` flag (#1673), thanks @davidsblom
- Added `DataEngineConfig.buffer_deltas` config option for the above (#1670), thanks @davidsblom
- Improved Bybit order book deltas parsing to set `F_LAST` flag (#1670), thanks @davidsblom
- Improved Bybit handling for top-of-book quotes and order book deltas (#1672), thanks @davidsblom
- Improved Interactive Brokers integration test mocks (#1669), thanks @rsmb7z
- Improved error message when no tick scheme initialized for an instrument, thanks for reporting @VeraLyu
- Improved `SandboxExecutionClient` instrument handling (instruments just need to be added to cache)
- Ported `VolumeWeightedAveragePrice` indicator to Rust (#1665), thanks @Pushkarm029
- Ported `VerticalHorizontalFilter` indicator to Rust (#1666), thanks @Pushkarm029

### Breaking Changes
None

### Fixes
- Fixed `SimulatedExchange` processing of commands in real-time for sandbox mode
- Fixed `DataEngine` unsubscribe handling (edge case would attempt to unsubscribe from the client multiple times)
- Fixed Bybit order book deltas parsing (was appending bid side twice) (#1668), thanks @davidsblom
- Fixed Binance instruments price and size precision parsing (was incorrectly stripping trailing zeros)
- Fixed `BinanceBar` streaming feather writing (was not setting up writer)
- Fixed backtest high-level tutorial documentation errors, thanks for reporting @Leonz5288

---

# NautilusTrader 1.193.0 Beta

Released on 24th May 2024 (UTC).

### Enhancements
- Added Interactive Brokers support for Market-on-Close (MOC) and Limit-on-Close (LOC) order types (#1663), thanks @rsmb7z
- Added Bybit sandbox example (#1659), thanks @davidsblom
- Added Binance sandbox example

### Breaking Changes
- Overhauled `SandboxExecutionClientConfig` to more closely match `BacktestVenueConfig` (many changes and additions)

### Fixes
- Fixed DataFusion backend data ordering by `ts_init` when streaming (#1656), thanks @twitu
- Fixed Interactive Brokers tick level historical data downloading (#1653), thanks @DracheShiki

---

# NautilusTrader 1.192.0 Beta

Released on 18th May 2024 (UTC).

### Enhancements
- Added Nautilus CLI (see [docs](https://nautilustrader.io/docs/nightly/developer_guide/index.html)) (#1602), many thanks @filipmacek
- Added `Cfd` and `Commodity` instruments with Interactive Brokers support (#1604), thanks @DracheShiki
- Added `OrderMatchingEngine` futures and option contract activation and expiration simulation
- Added Sandbox example with Interactive Brokers (#1618), thanks @rsmb7z
- Added `ParquetDataCatalog` S3 support (#1620), thanks @benjaminsingleton
- Added `Bar.from_raw_arrays_to_list` (#1623), thanks @rsmb7z
- Added `SandboxExecutionClientConfig.bar_execution` config option (#1646), thanks @davidsblom
- Improved venue order ID generation and assignment (it was previously possible for the `OrderMatchingEngine` to generate multiple IDs for the same order)
- Improved `LiveTimer` robustness and flexibility by not requiring positive intervals or stop times in the future (will immediately produce a time event), thanks for reporting @davidsblom

### Breaking Changes
- Removed `allow_cash_positions` config (simplify to the most common use case, spot trading should track positions)
- Changed `tags` parameter and return type from `str` to `list[str]` (more naturally expresses multiple tags)
- Changed `Order.to_dict()` `commission` and `linked_order_id` fields to lists of strings rather than comma separated strings
- Changed `OrderMatchingEngine` to no longer process internally aggregated bars for execution (no tests failed, but still classifying as a behavior change), thanks for reporting @davidsblom

### Fixes
- Fixed `CashAccount` PnL and balance calculations (was adjusting filled quantity based on open position quantity - causing a desync and incorrect balance values)
- Fixed `from_str` for `Price`, `Quantity` and `Money` when input string contains underscores in Rust, thanks for reporting @filipmacek
- Fixed `Money` string parsing where the value from `str(money)` can now be passed to `Money.from_str`
- Fixed `TimeEvent` equality (now based on the event `id` rather than the event `name`)
- Fixed `ParquetDataCatalog` bar queries by `instrument_id` which were no longer returning data (the intent is to use `bar_type`, however using `instrument_id` now returns all matching bars)
- Fixed venue order ID generation and application in sandbox mode (was previously generating additional venue order IDs), thanks for reporting @rsmb7z and @davidsblom
- Fixed multiple fills causing overfills in sandbox mode (`OrderMatchingEngine` now caching filled quantity to prevent this) (#1642), thanks @davidsblom
- Fixed `leaves_qty` exception message underflow (now correctly displays the projected negative leaves quantity)
- Fixed Interactive Brokers contract details parsing (#1615), thanks @rsmb7z
- Fixed Interactive Brokers portfolio registration (#1616), thanks @rsmb7z
- Fixed Interactive Brokers `IBOrder` attributes assignment (#1634), thanks @rsmb7z
- Fixed IBKR reconnection after gateway/TWS disconnection (#1622), thanks @benjaminsingleton
- Fixed Binance Futures account balance calculation (was over stating `free` balance with margin collateral, which could result in a negative `locked` balance)
- Fixed Betfair stream reconnection and avoid multiple reconnect attempts (#1644), thanks @imemo88

---

# NautilusTrader 1.191.0 Beta

Released on 20th April 2024 (UTC).

### Enhancements
- Implemented `FeeModel` including `FixedFeeModel` and `MakerTakerFeeModel` (#1584), thanks @rsmb7z
- Implemented `TradeTickDataWrangler.process_bar_data` (#1585), thanks @rsmb7z
- Implemented multiple timeframe bar execution (will use lowest timeframe per instrument)
- Optimized `LiveTimer` efficiency and accuracy with `tokio` timer under the hood
- Optimized `QuoteTickDataWrangler` and `TradeTickDataWrangler` (#1590), thanks @rsmb7z
- Standardized adapter client logging (handle more logging from client base classes)
- Simplified and consolidated Rust `OrderBook` design
- Improved `CacheDatabaseAdapter` graceful close and thread join
- Improved `MessageBus` graceful close and thread join
- Improved `modify_order` error logging when order values remain unchanged
- Added `RecordFlag` enum for Rust and Python
- Interactive Brokers further improvements and fixes, thanks @rsmb7z
- Ported `Bias` indicator to Rust, thanks @Pushkarm029

### Breaking Changes
- Reordered `OrderBookDelta` params `flags` and `sequence` and removed default 0 values (more explicit and less chance of mismatches)
- Reordered `OrderBook` params `flags` and `sequence` and removed default 0 values (more explicit and less chance of mismatches)
- Added `flags` parameter to `OrderBook.add`
- Added `flags` parameter to `OrderBook.update`
- Added `flags` parameter to `OrderBook.delete`
- Changed Arrow schema for all instruments: added `info` binary field
- Changed Arrow schema for `CryptoFuture`: added `is_inverse` boolean field
- Renamed both `OrderBookMbo` and `OrderBookMbp` to `OrderBook` (consolidated)
- Renamed `Indicator.handle_book_mbo` and `Indicator.handle_book_mbp` to `handle_book` (consolidated)
- Renamed `register_serializable_object` to `register_serializable_type` (also renames first parameter from `obj` to `cls`)

### Fixes
- Fixed `MessageBus` pattern resolving (fixes a performance regression where topics published with no subscribers would always re-resolve)
- Fixed `BacktestNode` streaming data management (was not clearing between chunks), thanks for reporting @dpmabo
- Fixed `RiskEngine` cumulative notional calculations for margin accounts (was incorrectly using base currency when selling)
- Fixed selling `Equity` instruments with `CASH` account and `NETTING` OMS incorrectly rejecting (should be able to reduce position)
- Fixed Databento bars decoding (was incorrectly applying display factor)
- Fixed `BinanceBar` (kline) to use `close_time` for `ts_event` was `opentime` (#1591), thanks for reporting @OnlyC
- Fixed `AccountMarginExceeded` error condition (margin must actually be exceeded now, and can be zero)
- Fixed `ParquetDataCatalog` path globbing which was including all paths with substrings of specified instrument IDs

---

# NautilusTrader 1.190.0 Beta

Released on 22nd March 2024 (UTC).

### Enhancements
- Added Databento adapter `continuous`, `parent` and `instrument_id` symbology support (will infer from symbols)
- Added `DatabaseConfig.timeout` config option for timeout seconds to wait for a new connection
- Added CSV tick and bar data loader params, thanks @rterbush
- Implemented `LogGuard` to ensure global logger is flushed on termination, thanks @ayush-sb and @twitu
- Improved Interactive Brokers client connectivity resilience and component lifecycle, thanks @benjaminsingleton
- Improved Binance execution client ping listen key error handling and logging
- Improved Redis cache adapter and message bus error handling and logging
- Improved Redis port parsing (`DatabaseConfig.port` can now be either a string or integer)
- Ported `ChandeMomentumOscillator` indicator to Rust, thanks @Pushkarm029
- Ported `VIDYA` indicator to Rust, thanks @Pushkarm029
- Refactored `InteractiveBrokersEWrapper`, thanks @rsmb7z
- Redact Redis passwords in strings and logs
- Upgraded `redis` crate to v0.25.2 which bumps up TLS dependencies, and turned on `tls-rustls-webpki-roots` feature flag

### Breaking Changes
None

### Fixes
- Fixed JSON format for log file output (was missing `timestamp` and `trader\_id`)
- Fixed `DatabaseConfig` port JSON parsing for Redis (was always defaulting to 6379)
- Fixed `ChandeMomentumOscillator` indicator divide by zero error (both Rust and Cython versions)

---

# NautilusTrader 1.189.0 Beta

Released on 15th March 2024 (UTC).

### Enhancements
- Implemented Binance order book snapshot rebuilds on websocket reconnect (see integration guide)
- Added additional validations for `OrderMatchingEngine` (will now raise a `RuntimeError` when a price or size precision for `OrderFilled` does not match the instruments precisions)
- Added `LoggingConfig.use_pyo3` config option for PyO3 based logging initialization (worse performance but allows visibility into logs originating from Rust)
- Added `exchange` field to `FuturesContract`, `FuturesSpread`, `OptionContract` and `OptionSpread` (optional)

### Breaking Changes
- Changed Arrow schema adding `exchange` field for `FuturesContract`, `FuturesSpread`, `OptionContract` and `OptionSpread`

### Fixes
- Fixed `MessageBus` handling of subscriptions after a topic has been published on (was previously dropping messages for these late subscribers)
- Fixed `MessageBus` handling of subscriptions under certain edge cases (subscriptions list could be resized on iteration causing a `RuntimeError`)
- Fixed `Throttler` handling of sending messages after messages have been dropped, thanks @davidsblom
- Fixed `OrderBookDelta.to_pyo3_list` using zero precision from clear delta
- Fixed `DataTransformer.pyo3_order_book_deltas_to_record_batch_bytes` using zero precision from clear delta
- Fixed `OrderBookMbo` and `OrderBookMbp` integrity check when crossed book
- Fixed `OrderBookMbp` error when attempting to add to a L1\_MBP book type (now raises `RuntimeError` rather than panicking)
- Fixed Interactive Brokers connection error logging (#1524), thanks @benjaminsingleton
- Fixed `SimulationModuleConfig` location and missing re-export from `config` subpackage
- Fixed logging `StdoutWriter` from also writing error logs (writers were duplicating error logs)
- Fixed `BinanceWebSocketClient` to [new specification](https://binance-docs.github.io/apidocs/futures/en/#websocket-market-streams) which requires responding to pings with a pong containing the pings payload
- Fixed Binance Futures `AccountBalance` calculations based on wallet and available balance
- Fixed `ExecAlgorithm` circular import issue for installed wheels (importing from `execution.algorithm` was a circular import)

---

# NautilusTrader 1.188.0 Beta

Released on 25th February 2024 (UTC).

### Enhancements
- Added `FuturesSpread` instrument type
- Added `OptionSpread` instrument type
- Added `InstrumentClass.FUTURE_SPREAD`
- Added `InstrumentClass.OPTION_SPREAD`
- Added `managed` parameter to `subscribe_order_book_deltas`, default `True` to retain current behavior (if false then the data engine will not automatically manage a book)
- Added `managed` parameter to `subscribe_order_book_snapshots`, default `True` to retain current behavior (if false then the data engine will not automatically manage a book)
- Added additional validations for `OrderMatchingEngine` (will now reject orders with incorrect price or quantity precisions)
- Removed `interval_ms` 20 millisecond limitation for `subscribe_order_book_snapshots` (i.e. just needs to be positive), although we recommend you consider subscribing to deltas below 100 milliseconds
- Ported `LiveClock` and `LiveTimer` implementations to Rust
- Implemented `OrderBookDeltas` pickling
- Implemented `AverageTrueRange` in Rust, thanks @rsmb7z

### Breaking Changes
- Changed `TradeId` value maximum length to 36 characters (will raise a `ValueError` if value exceeds the maximum)

### Fixes
- Fixed `TradeId` memory leak due assigning unique values to the `Ustr` global string cache (which are never freed for the lifetime of the program)
- Fixed `TradeTick` size precision for PyO3 conversion (size precision was incorrectly price precision)
- Fixed `RiskEngine` cash value check when selling (would previously divide quantity by price which is too much), thanks for reporting @AnthonyVince
- Fixed FOK time in force behavior (allows fills beyond the top level, will cancel if cannot fill full size)
- Fixed IOC time in force behavior (allows fills beyond the top level, will cancel any remaining after all fills are applied)
- Fixed `LiveClock` timer behavior for small intervals causing next time to be less than now (timer then would not run)
- Fixed log level filtering for `log_level_file` (bug introduced in v1.187.0), thanks @twitu
- Fixed logging `print_config` config option (was not being passed through to the logging subsystem)
- Fixed logging timestamps for backtesting (static clock was not being incrementally set to individual `TimeEvent` timestamps)
- Fixed account balance updates (fills from zero quantity `NETTING` positions will generate account balance updates)
- Fixed `MessageBus` publishable types collection type (needed to be `tuple` not `set`)
- Fixed `Controller` registration of components to ensure all active clocks are iterated correctly during backtests
- Fixed `Equity` short selling for `CASH` accounts (will now reject)
- Fixed `ActorFactory.create` JSON encoding (was missing the encoding hook)
- Fixed `ImportableConfig.create` JSON encoding (was missing the encoding hook)
- Fixed `ImportableStrategyConfig.create` JSON encoding (was missing the encoding hook)
- Fixed `ExecAlgorithmFactory.create` JSON encoding (was missing the encoding hook)
- Fixed `ControllerConfig` base class and docstring
- Fixed Interactive Brokers historical bar data bug, thanks @benjaminsingleton
- Fixed persistence `freeze_dict` function to handle `fs_storage_options`, thanks @dimitar-petrov

---

# NautilusTrader 1.187.0 Beta

Released on 9th February 2024 (UTC).

### Enhancements
- Refined logging subsystem module and writers in Rust, thanks @ayush-sb and @twitu
- Improved Interactive Brokers adapter symbology and parsing with a `strict_symbology` config option, thanks @rsmb7z and @fhill2

### Breaking Changes
- Reorganized configuration objects (separated into a `config` module per subpackage, with re-exports from `nautilus_trader.config`)

### Fixes
- Fixed `BacktestEngine` and `Trader` disposal (now properly releasing resources), thanks for reporting @davidsblom
- Fixed circular import issues from configuration objects, thanks for reporting @cuberone
- Fixed unnecessary creation of log files when file logging off

---

# NautilusTrader 1.186.0 Beta

Released on 2nd February 2024 (UTC).

### Enhancements
None

### Breaking Changes
None

### Fixes
- Fixed Interactive Brokers get account positions bug (#1475), thanks @benjaminsingleton
- Fixed `TimeBarAggregator` handling of interval types on build
- Fixed `BinanceSpotExecutionClient` non-existent method name, thanks @sunlei
- Fixed unused `psutil` import, thanks @sunlei

---

# NautilusTrader 1.185.0 Beta

Released on 26th January 2024 (UTC).

### Enhancements
- Added warning log when `bypass_logging` is set true for a `LIVE` context
- Improved `register_serializable object` to also add type to internal `_EXTERNAL_PUBLIHSABLE_TYPES`
- Improved Interactive Brokers expiration contract parsing, thanks @fhill2

### Breaking Changes
- Changed `StreamingConfig.include_types` type from `tuple[str]` to `list[type]` (better alignment with other type filters)
- Consolidated `clock` module into `component` module (reduce binary wheel size)
- Consolidated `logging` module into `component` module (reduce binary wheel size)

### Fixes
- Fixed Arrow serialization of `OrderUpdated` (`trigger_price` type was incorrect), thanks @benjaminsingleton
- Fixed `StreamingConfig.include_types` behavior (was not being honored for instrument writers), thanks for reporting @doublier1
- Fixed `ImportableStrategyConfig` type assignment in `StrategyFactory` (#1470), thanks @rsmb7z

---

# NautilusTrader 1.184.0 Beta

Released on 22nd January 2024 (UTC).

### Enhancements
- Added `LogLevel.OFF` (matches the Rust `tracing` log levels)
- Added `init_logging` function with sensible defaults to initialize the Rust implemented logging subsystem
- Updated Binance Futures enum members for `BinanceFuturesContractType` and `BinanceFuturesPositionUpdateReason`
- Improved log header using the `sysinfo` crate (adds swap space metrics and a PID identifier)
- Removed Python dependency on `psutil`

### Breaking Changes
- Removed `clock` parameter from `Logger` (no dependency on `Clock` anymore)
- Renamed `LoggerAdapter` to `Logger` (and removed old `Logger` class)
- Renamed `Logger` `component_name` parameter to `name` (matches Python built-in `logging` API)
- Renamed `OptionKind` `kind` parameter and property to `option_kind` (better clarity)
- Renamed `OptionContract` Arrow schema field `kind` to `option_kind`
- Changed `level_file` log level to `OFF` (file logging is off by default)

### Fixes
- Fixed memory leak for catalog queries (#1430), thanks @twitu
- Fixed `DataEngine` order book snapshot timer names (could not parse instrument IDs with hyphens), thanks for reporting @x-zho14 and @dimitar-petrov
- Fixed `LoggingConfig` parsing of `WARNING` log level (was not being recognized), thanks for reporting @davidsblom
- Fixed Binance Futures `QuoteTick` parsing to capture event time for `ts_event`, thanks for reporting @x-zho14

---

# NautilusTrader 1.183.0 Beta

Released on 12th January 2024 (UTC).

### Enhancements
- Added `NautilusConfig.json_primitives` to convert object to Python dictionary with JSON primitive values
- Added `InstrumentClass.BOND`
- Added `MessageBusConfig` `use_trader_prefix` and `use_trader_id` config options (provides more control over stream names)
- Added `CacheConfig.drop_instruments_on_reset` (default `True` to retain current behavior)
- Implemented core logging interface via the `log` crate, thanks @twitu
- Implemented global atomic clock in Rust (improves performance and ensures properly monotonic timestamps in real-time), thanks @twitu
- Improved Interactive Brokers adapter raising docker `RuntimeError` only when needed (not when using TWS), thanks @rsmb7z
- Upgraded core HTTP client to latest `hyper` and `reqwest`, thanks @ayush-sb
- Optimized Arrow encoding (resulting in ~100x faster writes for the Parquet data catalog)

### Breaking Changes
- Changed `ParquetDataCatalog` custom data prefix from `geneticdata_` to `custom_` (you will need to rename any catalog subdirs)
- Changed `ComponentStateChanged` Arrow schema for `config` from `string` to `binary`
- Changed `OrderInitialized` Arrow schema for `options` from `string` to `binary`
- Changed `OrderBookDeltas` dictionary representation of `deltas` field from JSON `bytes` to a list of `dict` (standardize with all other data types)
- Changed external message publishing stream name keys to be `trader-{trader_id}-{instance_id}-streams` (with options allows many traders to publish to the same streams)
- Renamed all version 2 data wrangler classes with a `V2` suffix for clarity
- Renamed `GenericData` to `CustomData` (more accurately reflects the nature of the type)
- Renamed `DataClient.subscribed_generic_data` to `.subscribed_custom_data`
- Renamed `MessageBusConfig.stream` to `.streams_prefix` (more accurate)
- Renamed `ParquetDataCatalog.generic_data` to `.custom_data`
- Renamed `TradeReport` to `FillReport` (more conventional terminology, and more clearly separates market data from user execution reports)
- Renamed `asset_type` to `instrument_class` across the codebase (more conventional terminology)
- Renamed `AssetType` enum to `InstrumentClass` (more conventional terminology)
- Renamed `AssetClass.BOND` to `AssetClass.DEBT` (more conventional terminology)
- Removed `AssetClass.METAL` (not strictly an asset class, more a futures category)
- Removed `AssetClass.ENERGY` (not strictly an asset class, more a futures category)
- Removed `multiplier` parameter from `Equity` constructor (not applicable)
- Removed `size_precision`, `size_increment`, and `multiplier` fields from `Equity` dictionary representation (not applicable)
- Removed `TracingConfig` (now redundant with new logging implementation)
- Removed `Ticker` data type and associated methods (not a type which can be practically normalized and so becomes adapter specific generic data)
- Moved `AssetClass.SPORTS_BETTING` to `InstrumentClass.SPORTS_BETTING`

### Fixes
- Fixed logger thread leak, thanks @twitu
- Fixed handling of configuration objects to work with `StreamingFeatherWriter`
- Fixed `BinanceSpotInstrumentProvider` fee loading key error for partial instruments load, thanks for reporting @doublier1
- Fixed Binance API key configuration parsing for testnet (was falling through to non-testnet env vars)
- Fixed TWAP execution algorithm scheduled size handling when first order should be for the entire size, thanks for reporting @pcgm-team
- Added `BinanceErrorCode.SERVER_BUSY` (-1008), also added to the retry error codes
- Added `BinanceOrderStatus.EXPIRED_IN_MATCH` which is when an order was canceled by the exchange due self-trade prevention (STP), thanks for reporting @doublier1

---

# NautilusTrader 1.182.0 Beta

Released on 23rd December 2023 (UTC).

### Enhancements
- Added `CacheDatabaseFacade` and `CacheDatabaseAdapter` to abstract backing technology from Python codebase
- Added `RedisCacheDatabase` implemented in Rust with separate MPSC channel thread for insert, update and delete operations
- Added TA-Lib integration, thanks @rsmb7z
- Added `OrderBookDelta` and `OrderBookDeltas` to serializable and publishable types
- Moved `PortfolioFacade` to `Actor`
- Improved `Actor` and `Strategy` usability to be more lenient to mistaken calls to `clock` and `logger` from the constructor (warnings also added to docs)
- Removed `redis` and `hiredis` dependencies from Python codebase

### Breaking Changes
- Changed configuration objects to take stronger types as these are now serializable when registered (rather than primitives)
- Changed `NautilusKernelConfig.trader_id` to type `TraderId`
- Changed `BacktestDataConfig.instrument_id` to type `InstrumentId`
- Changed `ActorConfig.component_id` to type `ComponentId | None`
- Changed `StrategyConfig.strategy_id` to type `StrategyId | None`
- Changed `Instrument`, `OrderFilled` and `AccountState` `info` field serialization due below fix (you'll need to flush your cache)
- Changed `CacheConfig` to take a `DatabaseConfig` (better symmetry with `MessageBusConfig`)
- Changed `RedisCacheDatabase` data structure for currencies from hashset to simpler key-value (you'll need to clear cache or delete all currency keys)
- Changed `Actor` state loading to now use the standard `Serializer`
- Renamed `register_json_encoding` to `register_config_encoding`
- Renamed `register_json_decoding` to `register_config_decoding`
- Removed `CacheDatabaseConfig` (due above config change)
- Removed `infrastructure` subpackage (now redundant with new Rust implementation)

### Fixes
- Fixed `json` encoding for `CacheDatabaseAdapter` from `info` field serialization fix below
- Fixed `Instrument`, `OrderFilled` and `AccountState` `info` field serialization to retain JSON serializable dicts (rather than double encoding and losing information)
- Fixed Binance Futures `good_till_date` value when `time_in_force` not GTD, such as when strategy is managing the GTD (was incorrectly passing through UNIX milliseconds)
- Fixed `Executor` handling of queued task IDs (was not discarding from queued tasks on completion)
- Fixed `DataEngine` handling of order book snapshots with very small intervals (now handles as short as 20 milliseconds)
- Fixed `BacktestEngine.clear_actors()`, `BacktestEngine.clear_strategies()` and `BacktestEngine.clear_exec_algorithms()`, thanks for reporting @davidsblom
- Fixed `BacktestEngine` OrderEmulator reset, thanks @davidsblom
- Fixed `Throttler.reset` and reset of `RiskEngine` throttlers, thanks @davidsblom

---

# NautilusTrader 1.181.0 Beta

Released on 2nd December (UTC).

This release adds support for Python 3.12.

### Enhancements
- Rewrote Interactive Brokers integration documentation, many thanks @benjaminsingleton
- Added Interactive Brokers adapter support for crypto instruments with cash quantity, thanks @benjaminsingleton
- Added `HistoricInteractiveBrokerClient`, thanks @benjaminsingleton and @limx0
- Added `DataEngineConfig.time_bars_interval_type` (determines the type of interval used for time aggregation `left-open` or `right-open`)
- Added `LoggingConfig.log_colors` to optionally use ANSI codes to produce colored logs (default `True` to retain current behavior)
- Added `QuoteTickDataWrangler.process_bar_data` options for `offset_interval_ms` and `timestamp_is_close`
- Added identifier generators in Rust, thanks @filipmacek
- Added `OrderFactory` in Rust, thanks @filipmacek
- Added `WilderMovingAverage` in Rust, thanks @ayush-sb
- Added `HullMovingAverage` in Rust, thanks @ayush-sb
- Added all common identifier generators in Rust, thanks @filipmacek
- Added generic SQL database support with `sqlx` in Rust, thanks @filipmacek

### Breaking Changes
- Consolidated all `data` submodules into one `data` module (reduce binary wheel size)
- Moved `OrderBook` from `model.orderbook.book` to `model.book` (subpackage only had this single module)
- Moved `Currency` from `model.currency` to `model.objects` (consolidating modules to reduce binary wheel size)
- Moved `MessageBus` from `common.msgbus` to `common.component` (consolidating modules to reduce binary wheel size)
- Moved `MsgSpecSerializer` from `serialization.msgpack.serializer` to `serialization.serializer`
- Moved `CacheConfig` `snapshot_orders`, `snapshot_positions`, `snapshot_positions_interval` to `NautilusKernelConfig` (logical applicability)
- Renamed `MsgPackSerializer` to `MsgSpecSeralizer` (now handles both JSON and MsgPack formats)

### Fixes
- Fixed missing `trader_id` in `Position` dictionary representation, thanks @filipmacek
- Fixed conversion of fixed-point integers to floats (should be dividing to avoid rounding errors), thanks for reporting @filipmacek
- Fixed daily timestamp parsing for Interactive Brokers, thanks @benjaminsingleton
- Fixed live reconciliation trade processing for partially filled then canceled orders
- Fixed `RiskEngine` cumulative notional risk check for `CurrencyPair` SELL orders on multi-currency cash accounts

---

# NautilusTrader 1.180.0 Beta

Released on 3rd November 2023 (UTC).

### Enhancements
- Improved internal latency for live engines by using `loop.call_soon_threadsafe(...)`
- Improved `RedisCacheDatabase` client connection error handling with retries
- Added `WebSocketClient` connection headers, thanks @ruthvik125 and @twitu
- Added `support_contingent_orders` config option for venues (to simulate venues which do not support contingent orders)
- Added `StrategyConfig.manage_contingent_orders` config option (to automatically manage **open** contingent orders)
- Added `FuturesContract.activation_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `OptionContract.activation_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `CryptoFuture.activation_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `FuturesContract.expiration_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `OptionContract.expiration_utc` property which returns a `pd.Timestamp` tz-aware (UTC)
- Added `CryptoFuture.expiration_utc` property which returns a `pd.Timestamp` tz-aware (UTC)

### Breaking Changes
- Renamed `FuturesContract.expiry_date` to `expiration_ns` (and associated params) as `uint64_t` UNIX nanoseconds
- Renamed `OptionContract.expiry_date` to `expiration_ns` (and associated params) as `uint64_t` UNIX nanoseconds
- Renamed `CryptoFuture.expiry_date` to `expiration_ns` (and associated params) as `uint64_t` UNIX nanoseconds
- Changed `FuturesContract` Arrow schema
- Changed `OptionContract` Arrow schema
- Changed `CryptoFuture` Arrow schema
- Transformed orders will now retain the original `ts_init` timestamp
- Removed unimplemented `batch_more` option for `Strategy.modify_order`
- Removed `InstrumentProvider.venue` property (redundant as a provider may have many venues)
- Dropped support for Python 3.9

### Fixes
- Fixed `ParquetDataCatalog` file writing template, thanks @limx0
- Fixed Binance all orders requests which would omit order reports when using a `start` param
- Fixed managed GTD orders past expiry cancellation on restart (orders were not being canceled)
- Fixed managed GTD orders cancel timer on order cancel (timers were not being canceled)
- Fixed `BacktestEngine` logging error with immediate stop (caused by certain timestamps being `None`)
- Fixed `BacktestNode` exceptions during backtest runs preventing next sequential run, thanks for reporting @cavan-black
- Fixed `BinanceSpotPermission` value error by relaxing typing for `BinanceSpotSymbolInfo.permissions`
- Interactive Brokers adapter various fixes, thanks @rsmb7z

---

# NautilusTrader 1.179.0 Beta

Released on 22nd October 2023 (UTC).

A major feature of this release is the `ParquetDataCatalog` version 2, which represents months of
collective effort thanks to contributions from Brad @limx0, @twitu, @ghill2 and @davidsblom.

This will be the final release with support for Python 3.9.

### Enhancements
- Added `ParquetDataCatalog` v2 supporting built-in data types `OrderBookDelta`, `QuoteTick`, `TradeTick` and `Bar`
- Added `Strategy` specific order and position event handlers
- Added `ExecAlgorithm` specific order and position event handlers
- Added `Cache.is_order_pending_cancel_local(...)` (tracks local orders in cancel transition)
- Added `BinanceTimeInForce.GTD` enum member (futures only)
- Added Binance Futures support for GTD orders
- Added Binance internal bar aggregation inference from aggregated trades or 1-MINUTE bars (depending on lookback window)
- Added `BinanceExecClientConfig.use_gtd` config option (to remap to GTC and locally manage GTD orders)
- Added package version check for `nautilus_ibapi`, thanks @rsmb7z
- Added `RiskEngine` min/max instrument notional limit checks
- Added `Controller` for dynamically controlling actor and strategy instances for a `Trader`
- Added `ReportProvider.generate_fills_report(...)` which provides a row per individual fill event, thanks @r3k4mn14r
- Moved indicator registration and data handling down to `Actor` (now available for `Actor`)
- Implemented Binance `WebSocketClient` live subscribe and unsubscribe
- Implemented `BinanceCommonDataClient` retries for `update_instruments`
- Decythonized `Trader`

### Breaking Changes
- Renamed `BookType.L1_TBBO` to `BookType.L1_MBP` (more accurate definition, as L1 is the top-level price either side)
- Renamed `VenueStatusUpdate` -> `VenueStatus`
- Renamed `InstrumentStatusUpdate` -> `InstrumentStatus`
- Renamed `Actor.subscribe_venue_status_updates(...)` to `Actor.subscribe_venue_status(...)`
- Renamed `Actor.subscribe_instrument_status_updates(...)` to `Actor.subscribe_instrument_status(...)`
- Renamed `Actor.unsubscribe_venue_status_updates(...)` to `Actor.unsubscribe_venue_status(...)`
- Renamed `Actor.unsubscribe_instrument_status_updates(...)` to `Actor.unsubscribe_instrument_status(...)`
- Renamed `Actor.on_venue_status_update(...)` to `Actor.on_venue_status(...)`
- Renamed `Actor.on_instrument_status_update(...)` to `Actor.on_instrument_status(...)`
- Changed `InstrumentStatus` fields/schema and constructor
- Moved `manage_gtd_expiry` from `Strategy.submit_order(...)` and `Strategy.submit_order_list(...)` to `StrategyConfig` (simpler and allows re-activating any GTD timers on start)

### Fixes
- Fixed `LimitIfTouchedOrder.create` (`exec_algorithm_params` were not being passed in)
- Fixed `OrderEmulator` start-up processing of OTO contingent orders (when position from parent is open)
- Fixed `SandboxExecutionClientConfig` `kw_only=True` to allow importing without initializing
- Fixed `OrderBook` pickling (did not include all attributes), thanks @limx0
- Fixed open position snapshots race condition (added `open_only` flag)
- Fixed `Strategy.cancel_order` for orders in `INITIALIZED` state and with an `emulation_trigger` (was not sending command to `OrderEmulator`)
- Fixed `BinanceWebSocketClient` reconnect behavior (reconnect handler was not being called due event loop issue from Rust)
- Fixed Binance instruments missing max notional values, thanks for reporting @AnthonyVince and thanks for fixing @filipmacek
- Fixed Binance Futures fee rates for backtesting
- Fixed `Timer` missing condition check for non-positive intervals
- Fixed `Condition` checks involving integers, was previously defaulting to 32-bit and overflowing
- Fixed `ReportProvider.generate_order_fills_report(...)` which was missing partial fills for orders not in a final `FILLED` status, thanks @r3k4mn14r

---

# NautilusTrader 1.178.0 Beta

Released on 2nd September 2023 (UTC).

### Enhancements
None

### Breaking Changes
None

### Fixes
- Fixed `OrderBookDelta.clear` method (where the `sequence` field was swapped with `flags` causing an overflow)
- Fixed `OrderManager` OTO contingency handling on fills
- Fixed `OrderManager` duplicate order canceled events (race condition when processing contingencies)
- Fixed `Cache` loading of initialized emulated orders (were not being correctly indexed as emulated)
- Fixed Binance order book subscriptions for deltas at full depth (was not requesting initial snapshot), thanks for reporting @doublier1

---

# NautilusTrader 1.177.0 Beta

Released on 26th August 2023 (UTC).

This release includes a large breaking change to quote tick bid and ask price property and
parameter naming. This was done in the interest of maintaining our generally explicit naming
standards, and has caused confusion for some users in the past. Data using 'bid' and 'ask' columns should
still work with the legacy data wranglers, as columns are renamed under the hood to accommodate
this change.

### Enhancements
- Added `ActorExecutor` with `Actor` API for creating and running threaded tasks in live environments
- Added `OrderEmulated` event and associated `OrderStatus.EMULATED` enum variant
- Added `OrderReleased` event and associated `OrderStatus.RELEASED` enum variant
- Added `BacktestVenueConfig.use_position_ids` config option (default `True` to retain current behavior)
- Added `Cache.exec_spawn_total_quantity(...)` convenience method
- Added `Cache.exec_spawn_total_filled_qty(...)` convenience method
- Added `Cache.exec_spawn_total_leaves_qty(...)` convenience method
- Added `WebSocketClient.send_text`, thanks @twitu
- Implemented string interning for `TimeEvent`

### Breaking Changes
- Renamed `QuoteTick.bid` to `bid_price` including all associated parameters (for explicit naming standards)
- Renamed `QuoteTick.ask` to `ask_price` including all associated parameters (for explicit naming standards)

### Fixes
- Fixed execution algorithm `position_id` assignment in `HEDGING` mode
- Fixed `OrderMatchingEngine` processing of emulated orders
- Fixed `OrderEmulator` processing of exec algorithm orders
- Fixed `ExecutionEngine` processing of exec algorithm orders (exec spawn IDs)
- Fixed `Cache` emulated order indexing (were not being properly discarded from the set when closed)
- Fixed `RedisCacheDatabase` loading of transformed `LIMIT` orders
- Fixed a connection issue with the IB client, thanks @dkharrat and @rsmb7z

---

# NautilusTrader 1.176.0 Beta

Released on 31st July 2023 (UTC).

### Enhancements
- Implemented string interning with the [ustr](https://github.com/anderslanglands/ustr) crate, thanks @twitu
- Added `SyntheticInstrument` capability, including dynamic derivation formulas
- Added `Order.commissions()` convenience method (also added to state snapshot dictionaries)
- Added `Cache` position and order state snapshots (configure via `CacheConfig`)
- Added `CacheDatabaseConfig.timestamps_as_iso8601` to persist timestamps as ISO 8601 strings
- Added `LiveExecEngineConfig.filter_position_reports` to filter position reports from reconciliation
- Added `Strategy.cancel_gtd_expiry` to cancel managed GTD order expiration
- Added Binance Futures support for modifying `LIMIT` orders
- Added `BinanceExecClientConfig.max_retries` config option (for retrying order submit and cancel requests)
- Added `BinanceExecClientConfig.retry_delay` config option (the delay between retry attempts)
- Added `BinanceExecClientConfig.use_reduce_only` config option (default `True` to retain current behavior)
- Added `BinanceExecClientConfig.use_position_ids` config option (default `True` to retain current behavior)
- Added `BinanceExecClientConfig.treat_expired_as_canceled` option (default `False` to retain current behavior)
- Added `BacktestVenueConfig.use_reduce_only` config option (default `True` to retain current behavior)
- Added `MessageBus.is_pending_request(...)` method
- Added `Level` API for core `OrderBook` (exposes the bid and ask levels for the order book)
- Added `Actor.is_pending_request(...)` convenience method
- Added `Actor.has_pending_requests()` convenience method
- Added `Actor.pending_requests()` convenience method
- Added `USDP` (Pax Dollar) and `TUSD` (TrueUSD) stablecoins
- Improved `OrderMatchingEngine` handling when no fills (an error is now logged)
- Improved Binance live clients logging
- Upgraded Cython to v3.0.0 stable

### Breaking Changes
- Moved `filter_unclaimed_external_orders` from `ExecEngineConfig` to `LiveExecEngineConfig`
- All `Actor.request_*` methods no longer take a `request_id`, but now return a `UUID4` request ID
- Removed `BinanceExecClientConfig.warn_gtd_to_gtd` (now always an `INFO` level log)
- Renamed `Instrument.native_symbol` to `raw_symbol` (you must manually migrate or flush your cached instruments)
- Renamed `Position.cost_currency` to `settlement_currency` (standardize terminology)
- Renamed `CacheDatabaseConfig.flush` to `flush_on_start` (for clarity)
- Changed `Order.ts_last` to represent the UNIX nanoseconds timestamp of the last _event_ (rather than fill)

### Fixes
- Fixed `Portfolio.net_position` calculation to use `Decimal` rather than `float` to avoid rounding errors
- Fixed race condition on `OrderFactory` order identifiers generation
- Fixed dictionary representation of orders for `venue_order_id` (for three order types)
- Fixed `Currency` registration with core global map on creation
- Fixed serialization of `OrderInitialized.exec_algorithm_params` to spec (bytes rather than string)
- Fixed assignment of position IDs for contingent orders (when parent filled)
- Fixed `PENDING_CANCEL` -> `EXPIRED` as valid state transition (real world possibility)
- Fixed fill handling of `reduce_only` orders when partially filled
- Fixed Binance reconciliation which was requesting reports for the same symbol multiple times
- Fixed Binance Futures native symbol parsing (was actually Nautilus symbol values)
- Fixed Binance Futures `PositionStatusReport` parsing of position side
- Fixed Binance Futures `TradeReport` assignment of position ID (was hardcoded to hedging mode)
- Fixed Binance execution submitting of order lists
- Fixed Binance commission rates requests for `InstrumentProvider`
- Fixed Binance `TriggerType` parsing #1154, thanks for reporting @davidblom603
- Fixed Binance order parsing of invalid orders in execution reports #1157, thanks for reporting @graceyangfan
- Extended `BinanceOrderType` enum members to include undocumented `INSURANCE_FUND`, thanks for reporting @Tzumx
- Extended `BinanceSpotPermissions` enum members #1161, thanks for reporting @davidblom603

---

# NautilusTrader 1.175.0 Beta

Released on 16th June 2023 (UTC).

The Betfair adapter is broken for this release pending integration with the new Rust order book.
We recommend you do not upgrade to this version if you're using the Betfair adapter.

### Enhancements
- Integrated Interactive Brokers adapter v2 into platform, thanks @rsmb7z
- Integrated core Rust `OrderBook` into platform
- Integrated core Rust `OrderBookDelta` data type
- Added core Rust `HttpClient` based on `hyper`, thanks @twitu
- Added core Rust `WebSocketClient` based on `tokio-tungstenite`, thanks @twitu
- Added core Rust `SocketClient` based on `tokio` `TcpStream`, thanks @twitu
- Added `quote_quantity` parameter to determine if order quantity is denominated in quote currency
- Added `trigger_instrument_id` parameter to trigger emulated orders from alternative instrument prices
- Added `use_random_ids` to `add_venue(...)` method, controls whether venue order, position and trade IDs will be random UUID4s (no change to current behavior)
- Added `ExecEngineConfig.filter_unclaimed_external_orders` config option, if unclaimed order events with an `EXTERNAL` strategy ID should be filtered/dropped
- Changed `BinanceHttpClient` to use new core HTTP client
- Defined public API for data, can now import directly from `nautilus_trader.model.data` (denest namespace)
- Defined public API for events, can now import directly from `nautilus_trader.model.events` (denest namespace)

### Breaking Changes
- Upgraded `pandas` to v2
- Removed `OrderBookSnapshot` (redundant as can be represented as an initial CLEAR followed by deltas)
- Removed `OrderBookData` (redundant)
- Renamed `Actor.handle_order_book_delta` to `handle_order_book_deltas` (to more clearly reflect the `OrderBookDeltas` data type)
- Renamed `Actor.on_order_book_delta` to `on_order_book_deltas` (to more clearly reflect the `OrderBookDeltas` data type)
- Renamed `inverse_as_quote` to `use_quote_for_inverse` (ambiguous name, only applicable for notional calcs on inverse instruments)
- Changed `Data` contract (custom data), [see docs](https://nautilustrader.io/docs/latest/concepts/advanced/data.html)
- Renamed core `LogMessage` to `LogEvent` to more clearly distinguish between the `message` field and the event struct itself (aligns with [vector](https://vector.dev/docs/about/under-the-hood/architecture/data-model/log/) language)
- Renamed core `LogEvent.timestamp_ns` to `LogEvent.timestamp` (affects field name for JSON format)
- Renamed core `LogEvent.msg` to `LogEvent.message` (affects field name for JSON format)

### Fixes
- Updated `BinanceAccountType` enum members and associated docs
- Fixed `BinanceCommonExecutionClient` iteration of `OrderList` orders
- Fixed heartbeats for `BinanceWebSocketClient` (new Rust client now responds with `pong` frames)
- Fixed Binance adapter typing for `orderId`, `fromId`, `startTime` and `endTime` (all are ints), thanks for reporting @davidsblom
- Fixed `Currency` equality to be based on the `code` field (avoiding equality issues over FFI), thanks for reporting @Otlk
- Fixed `BinanceInstrumentProvider` parsing of initial and maintenance margin values

---

# NautilusTrader 1.174.0 Beta

Released on 19th May 2023 (UTC).

### Breaking Changes
- Parquet schemas are now shifting towards catalog v2 (we recommend you don't upgrade if using legacy catalog)
- Moved order book data from `model.orderbook.data` into the `model.data.book` namespace

### Enhancements
- Improved handling for backtest account blow-up scenarios (balance negative or margin exceeded)
- Added `AccountMarginExceeded` exception and refined `AccountBalanceNegative`
- Various improvements to Binance clients error handling and logging
- Improve Binance HTTP error messages

### Fixes
- Fixed handling of emulated order contingencies (not based on status of spawned algorithm orders)
- Fixed sending execution algorithm commands from strategy
- Fixed `OrderEmulator` releasing of already closed orders
- Fixed `MatchingEngine` processing of reduce only for child contingent orders
- Fixed `MatchingEngine` position ID assignment for child contingent orders
- Fixed `Actor` handling of historical data from requests (will now call `on_historical_data` regardless of state), thanks for reporting @miller-moore
- Fixed `pyarrow` schema dictionary index keys being too narrow (int8 -> int16), thanks for reporting @rterbush

---

# NautilusTrader 1.173.0 Beta

Released on 5th May 2023 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed `BacktestEngine` processing of venue(s) message queue based off time event `ts_init`
- Fixed `Position.signed_decimal_qty` (incorrect format precision in f-string), thanks for reporting @rsmb7z
- Fixed trailing stop type order updates for `reduce_only` instruction, thanks for reporting @Otlk
- Fixed updating of active execution algorithm orders (events weren't being cached)
- Fixed condition check for applying pending events (do not apply to orders at `INITIALIZED` status)

---

# NautilusTrader 1.172.0 Beta

Released on 30th April 2023 (UTC).

### Breaking Changes
- Removed legacy Rust parquet data catalog backend (based on arrow2)
- Removed Binance config for `clock_sync_interval_secs` (redundant/unused and should be handled at system level)
- Removed redundant rate limiting from Rust logger (and associated `rate_limit` config params)
- Renamed `Future` instrument to `FuturesContract` (avoids ambiguity)
- Renamed `Option` instrument to `OptionContract` (avoids ambiguity and naming conflicts in Rust)
- Reinstate hours and minutes time component for default order and position identifiers (easier debugging, less collisions)
- Setting time alerts for in the past or current time will generate an immediate `TimeEvent` (rather than being invalid)

### Enhancements
- Added new DataFusion Rust parquet data catalog backend (yet to be integrated into Python)
- Added `external_order_claims` config option for `StrategyConfig` (for claiming external orders per instrument)
- Added `Order.signed_decimal_qty()`
- Added `Cache.orders_for_exec_algorithm(...)`
- Added `Cache.orders_for_exec_spawn(...)`
- Added `TWAPExecAlgorithm` and `TWAPExecAlgorithmConfig` to examples
- Build out `ExecAlgorithm` base class for implementing 'first class' execution algorithms
- Rewired execution for improved flow flexibility between emulated orders, execution algorithms and the `RiskEngine`
- Improved handling for `OrderEmulator` updating of contingent orders from execution algorithms
- Defined public API for instruments, can now import directly from `nautilus_trader.model.instruments` (denest namespace)
- Defined public API for orders, can now import directly from `nautilus_trader.model.orders` (denest namespace)
- Defined public API for order book, can now import directly from `nautilus_trader.model.orderbook` (denest namespace)
- Now stripping debug symbols after build (reduced binary wheel size)
- Refined build and added additional `debug` Makefile convenience targets

### Fixes
- Fixed processing of contingent orders when in a pending update state
- Fixed calculation of PnL for flipped positions (only book realized PnL against open position)
- Fixed `WebSocketClient` session disconnect, thanks for reporting @miller-moore
- Added missing `BinanceSymbolFilterType.NOTIONAL`
- Fixed incorrect `Mul` trait for `Price` and `Quantity` (not being used in Cython/Python layer)

---

# NautilusTrader 1.171.0 Beta

Released on 30th March 2023 (UTC).

### Breaking Changes
- Renamed all position `net_qty` fields and parameters to `signed_qty` (more accurate naming)
- `NautilusKernelConfig` removed all `log_*` config options (replaced by `logging` with `LoggingConfig`)
- Trading `CurrencyPair` instruments with a _single-currency_ `CASH` account type no longer permitted (unrealistic)
- Changed `PositionEvent` parquet schemas (renamed `net_qty` field to `signed_qty`)

### Enhancements
- Added `LoggingConfig` to consolidate logging configs, offering various file options and per component level filters
- Added `BacktestVenueConfig.bar_execution` to control whether bar data moves the matching engine markets (reinstated)
- Added optional `request_id` for actor data requests (aids processing responses), thanks @rsmb7z
- Added `Position.signed_decimal_qty()`
- Now using above signed quantity for `Portfolio` net position calculation, and `LiveExecutionEngine` reconciliation comparisons

### Fixes
- Fixed `BacktestEngine` clock and logger handling (had a redundant extra logger and not swapping live clock in post run)
- Fixed `close_position` order event publishing and cache persistence for `MarketOrder` and `SubmitOrder`, thanks for reporting @rsmb7z

---

# NautilusTrader 1.170.0 Beta

Released on 11th March 2023 (UTC).

### Breaking Changes
- Moved `backtest.data.providers` to `test_kit.providers`
- Moved `backtest.data.wranglers` to `persistence.wranglers` (to be consolidated)
- Moved `backtest.data.loaders` to `persistence.loaders` (to be consolidated)
- Renamed `from_datetime` to `start` across data request methods and properties
- Renamed `to_datetime` to `end` across data request methods and properties
- Removed `RiskEngineConfig.deny_modify_pending_update` (as now redundant with new pending event sequencing)
- Removed redundant log sink machinery
- Changed parquet catalog schema dictionary integer key widths/types
- Invalidated all pickled data due to Cython 3.0.0b1 upgrade

### Enhancements
- Added logging to file at core Rust level
- Added `DataCatalogConfig` for more cohesive data catalog configuration
- Added `DataEngine.register_catalog` to support historical data requests
- Added `catalog_config` field to base `NautilusKernelConfig`
- Changed to immediately caching orders and order lists in `Strategy`
- Changed to checking duplicate `client_order_id` and `order_list_id` in `Strategy`
- Changed generating and applying `OrderPendingUpdate` and `OrderPendingCancel` in `Strategy`
- `PortfolioAnalyzer` PnL statistics now take optional `unrealized_pnl`
- Backtest performance statistics now include unrealized PnL in total PnL

### Fixes
- Fixed Binance Futures trigger type parsing
- Fixed `DataEngine` bar subscribe and unsubscribe logic, thanks for reporting @rsmb7z
- Fixed `Actor` handling of bars, thanks @limx0
- Fixed `CancelAllOrders` command handling for contingent orders not yet in matching core
- Fixed `TrailingStopMarketOrder` slippage calculation when no `trigger_price`, thanks for reporting @rsmb7z
- Fixed `BinanceSpotInstrumentProvider` parsing of quote asset (was using base), thanks for reporting @logogin
- Fixed undocumented Binance time in force 'GTE\_GTC', thanks for reporting @graceyangfan
- Fixed `Position` calculation of `last_qty` when commission currency was equal to base currency, thanks for reporting @rsmb7z
- Fixed `BacktestEngine` post backtest run PnL performance statistics for currencies traded per venue, thanks for reporting @rsmb7z

---

# NautilusTrader 1.169.0 Beta

Released on 18th February 2023 (UTC).

### Breaking Changes
- `NautilusConfig` objects now _pseudo-immutable_ from new msgspec 0.13.0
- Renamed `OrderFactory.bracket` parameter `post_only_entry` -> `entry_post_only` (consistency with other params)
- Renamed `OrderFactory.bracket` parameter `post_only_tp` -> `tp_post_only` (consistency with other params)
- Renamed `build_time_bars_with_no_updates` -> `time_bars_build_with_no_updates` (consistency with new param)
- Renamed `OrderFactory.set_order_count()` -> `set_client_order_id_count()` (clarity)
- Renamed `TradingNode.start()` to `TradingNode.run()`

### Enhancements
- Complete overhaul and improvements to Binance adapter(s), thanks @poshcoe
- Added Binance aggregated trades functionality with `use_agg_trade_ticks`, thanks @poshcoe
- Added `time_bars_timestamp_on_close` config option for bar timestamping (`True` by default)
- Added `OrderFactory.generate_client_order_id()` (calls internal generator)
- Added `OrderFactory.generate_order_list_id()` (calls internal generator)
- Added `OrderFactory.create_list(...)` as easier method for creating order lists
- Added `__len__` implementation for `OrderList` (returns length of orders)
- Implemented optimized logger using Rust MPSC channel and separate thread
- Expose and improve `MatchingEngine` public API for custom functionality
- Exposed `TradingNode.run_async()` for easier running from async context
- Exposed `TradingNode.stop_async()` for easier stopping from async context

### Fixes
- Fixed registration of `SimulationModule` (and refine `Actor` base registration)
- Fixed loading of previously emulated and transformed orders (handles transforming `OrderInitialized` event)
- Fixed handling of `MARKET_TO_LIMIT` orders in matching and risk engines, thanks for reporting @martinsaip

---

# NautilusTrader 1.168.0 Beta

Released on 29th January 2023 (UTC).

### Breaking Changes
- Removed `Cache.clear_cache()` (redundant with the `.reset()` method)

### Enhancements
- Added `Cache` `.add(...)` and `.get(...)` for general 'user/custom' objects (as bytes)
- Added `CacheDatabase` `.add(...)` and `.load()` for general cache objects (as bytes)
- Added `RedisCacheDatabase` `.add(...) `and `.load()` for general Redis persisted bytes objects (as bytes)
- Added `Cache.actor_ids()`
- Added `Actor` cached state saving and loading functionality
- Improved logging for called action handlers when not overridden

### Fixes
- Fixed configuration of loading and saving actor and strategy state

---

# NautilusTrader 1.167.0 Beta

Released on 28th January 2023 (UTC).

### Breaking Changes
- Renamed `OrderBookData.update_id` to `sequence`
- Renamed `BookOrder.id` to `order_id`

### Enhancements
- Introduced Rust PyO3 based `ParquetReader` and `ParquetWriter`, thanks @twitu
- Added `msgbus.is_subscribed` (to check if topic and handler already subscribed)
- Simplified message type model and introduce CQRS-ish live messaging architecture

### Fixes
- Fixed Binance data clients order book startup buffer handling
- Fixed `NautilusKernel` redundant initialization of event loop for backtesting, thanks @limx0
- Fixed `BacktestNode` disposal sequence
- Fixed quick start docs and notebook

---

# NautilusTrader 1.166.0 Beta

Released on 17th January 2023 (UTC).

### Breaking Changes
- `Position.unrealized_pnl` now `None` until any realized PnL is generated (to reduce ambiguity)

### Enhancements
- Added instrument status update subscription handlers, thanks @limx0
- Improvements to InteractiveBrokers `DataClient`, thanks @rsmb7z
- Improvements to async task handling for live clients
- Various improvements to Betfair adapter, thanks @limx0

### Fixes
- Fixed netted `Position` `realized_pnl` and `realized_return` fields, which were incorrectly cumulative
- Fixed netted `Position` flip logic (now correctly 'resets' position)
- Various fixes for Betfair adapter, thanks @limx0
- InteractiveBrokers integration docs fixes

---

# NautilusTrader 1.165.0 Beta

Released on 14th January 2023 (UTC).

A number of enum variant names have been changed in favour of explicitness,
and also to avoid C naming collisions.

### Breaking Changes
- Renamed `AggressorSide.NONE` to `NO_AGGRESSOR`
- Renamed `AggressorSide.BUY` to `BUYER`
- Renamed `AggressorSide.SELL` to `SELLER`
- Renamed `AssetClass.CRYPTO` to `CRYPTOCURRENCY`
- Renamed `LiquiditySide.NONE` to `NO_LIQUIDITY_SIDE`
- Renamed `OMSType` to `OmsType`
- Renamed `OmsType.NONE` to `UNSPECIFIED`
- Renamed `OrderSide.NONE` to `NO_ORDER_SIDE`
- Renamed `PositionSide.NONE` to `NO_POSITION_SIDE`
- Renamed `TrailingOffsetType.NONE` to `NO_TRAILING_OFFSET`
- Removed `TrailingOffsetType.DEFAULT`
- Renamed `TriggerType.NONE` to `NO_TRIGGER`
- Renamed `TriggerType.LAST` to `LAST_TRADE`
- Renamed `TriggerType.MARK` to `MARK_PRICE`
- Renamed `TriggerType.INDEX` to `INDEX_PRICE`
- Renamed `ComponentState.INITIALIZED` to `READY`
- Renamed `OrderFactory.bracket(post_only)` to `post_only_entry`
- Moved `manage_gtd_expiry` to `Strategy.submit_order(...)` and `Strategy.submit_order_list(...)`

### Enhancements
- Added `BarSpecification.timedelta` property, thanks @rsmb7z
- Added `DataEngineConfig.build_time_bars_with_no_updates` config option
- Added `OrderFactory.bracket(post_only_tp)` param
- Added `OrderListIdGenerator` and integrate with `OrderFactory`
- Added `Cache.add_order_list(...)`
- Added `Cache.order_list(...)`
- Added `Cache.order_lists(...)`
- Added `Cache.order_list_exists(...)`
- Added `Cache.order_list_ids(...)`
- Improved generation of `OrderListId` from factory to ensure uniqueness
- Added auction matches for backtests, thanks @limx0
- Added `.timedelta` property to `BarSpecification`, thanks @rsmb7z
- Numerous improvements to the Betfair adapter, thanks @limx0
- Improvements to Interactive Brokers data subscriptions, thanks @rsmb7z
- Added `DataEngineConfig.validate_data_sequence` (default `False` and currently only for `Bar` data), thanks @rsmb7z

### Fixes
- Added `TRD_GRP_*` enum variants for Binance spot permissions
- Fixed `PARTIALLY_FILLED` -> `EXPIRED` order state transition, thanks @bb01100100

---

# NautilusTrader 1.164.0 Beta

Released on 23rd December 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Added managed GTD order expiry (experimental feature, config may change)
- Added Rust `ParquetReader` and `ParquetWriter` (for `QuoteTick` and `TradeTick` only)

### Fixes
- Fixed `MARKET_IF_TOUCHED` orders for `OrderFactory.bracket(..)`
- Fixed `OrderEmulator` trigger event handling for live trading
- Fixed `OrderEmulator` transformation to market orders which had a GTD time in force
- Fixed serialization of `OrderUpdated` events
- Fixed typing and edge cases for new `msgspec`, thanks @limx0
- Fixed data wrangler processing with missing data, thanks @rsmb7z

---

# NautilusTrader 1.163.0 Beta

Released on 17th December 2022 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed `MARKET_IF_TOUCHED` and `LIMIT_IF_TOUCHED` trigger and modify behavior
- Fixed `MatchingEngine` updates of stop order types
- Fixed combinations of passive or immediate trigger vs passive or immediate fill behavior
- Fixed memory leaks from passing string pointers from Rust, thanks @twitu

---

# NautilusTrader 1.162.0 Beta

Released on 12th December 2022 (UTC).

### Breaking Changes
- `OrderFactory` bracket order methods consolidated to `.bracket(...)`

### Enhancements
- Extended `OrderFactory` to provide more bracket order types
- Simplified GitHub CI and removed `nox` dependency

### Fixes
- Fixed `OrderBook` sorting for bid side, thanks @gaugau3000
- Fixed `MARKET_TO_LIMIT` order initial fill behavior
- Fixed `BollingerBands` indicator mid-band calculations, thanks zhp (Discord)

---

# NautilusTrader 1.161.0 Beta

Released on 10th December 2022 (UTC).

This release adds support for Python 3.11.

### Breaking Changes
- Renamed `OrderFactory.bracket_market` to `OrderFactory.bracket_market_entry`
- Renamed `OrderFactory.bracket_limit` to `OrderFactory.bracket_limit_entry`
- Renamed `OrderFactory` bracket order `price` and `trigger_price` parameters

### Enhancements
- Consolidated config objects to `msgspec` providing better performance and correctness
- Added `OrderFactory.bracket_stop_limit_entry_stop_limit_tp(...)`
- Numerous improvements to the Interactive Brokers adapter, thanks @limx0 and @rsmb7z
- Removed dependency on `pydantic`

### Fixes
- Fixed `STOP_MARKET` order behavior to fill at market on immediate trigger
- Fixed `STOP_LIMIT` order behavior to fill at market on immediate trigger and marketable
- Fixed `STOP_LIMIT` order behavior to fill at market on processed trigger and marketable
- Fixed `LIMIT_IF_TOUCHED` order behavior to fill at market on immediate trigger and marketable
- Fixed Binance start and stop time units for bar (kline) requests, thanks @Tzumx
- `RiskEngineConfig.bypass` set to `True` will now correctly bypass throttlers, thanks @DownBadCapital
- Fixed updating of emulated orders
- Numerous fixes to the Interactive Brokers adapter, thanks @limx0 and @rsmb7z

---

# NautilusTrader 1.160.0 Beta

Released on 28th November 2022 (UTC).

### Breaking Changes
- Removed time portion from generated IDs (affects `ClientOrderId` and `PositionOrderId`)
- Renamed `orderbook.data.Order` to `orderbook.data.BookOrder` (reduce conflicts/confusion)
- Renamed `Instrument.get_cost_currency(...)` to `Instrument.get_settlement_currency(...)` (more accurate terminology)

### Enhancements
- Added emulated contingent orders capability to `OrderEmulator`
- Moved `test_kit` module to main package to support downstream project/package testing

### Fixes
- Fixed position event sequencing: now generates `PositionOpened` when reopening a closed position
- Fixed `LIMIT` order fill characteristics when immediately marketable as a taker
- Fixed `LIMIT` order fill characteristics when passively filled as a maker as quotes move through
- Fixed canceling OTO contingent orders when still in-flight
- Fixed `RiskEngine` notional check when selling cash assets (spot currency pairs)
- Fixed flush on closed file bug for persistence stream writers

---

# NautilusTrader 1.159.0 Beta

Released on 18th November 2022 (UTC).

### Breaking Changes
- Removed FTX integration
- Renamed `SubmitOrderList.list` to `SubmitOrderList.order_list`
- Slight adjustment to bar aggregation (will not use the last close as the open)

### Enhancements
- Implemented `TRAILING_STOP_MARKET` orders for Binance Futures (beta)
- Added `OUO` One-Updates-Other `ContingencyType` with matching engine implementation
- Added bar price fallback for exchange rate calculations, thanks @ghill2

### Fixes
- Fixed dealloc of Rust backing struct on Python exceptions causing segfaults
- Fixed bar aggregation start times for bar specs outside typical intervals (60-SECOND rather than 1-MINUTE etc)
- Fixed backtest engine main loop ordering of time events with identically timestamped data
- Fixed `ModifyOrder` message `str` and `repr` when no quantity
- Fixed OCO contingent orders which were actually implemented as OUO for backtests
- Fixed various bugs for Interactive Brokers integration, thanks @limx0 and @rsmb7z
- Fixed pyarrow version parsing, thanks @ghill2
- Fixed returning venue from InstrumentId, thanks @rsmb7z

---

# NautilusTrader 1.158.0 Beta

Released on 3rd November 2022 (UTC).

### Breaking Changes
- Added `LiveExecEngineConfig.reconciliation` boolean flag to control if reconciliation is active
- Removed `LiveExecEngineConfig.reconciliation_auto` (unclear naming and concept)
- All Redis keys have changed to a lowercase convention (either migrate or flush your Redis)
- Removed `BidAskMinMax` indicator (to reduce total package size)
- Removed `HilbertPeriod` indicator (to reduce total package size)
- Removed `HilbertSignalNoiseRatio` indicator (to reduce total package size)
- Removed `HilbertTransform` indicator (to reduce total package size)

### Enhancements
- Improved accuracy of clocks for backtests (all clocks will now match generated `TimeEvent`s)
- Improved risk engine checks for `reduce_only` orders
- Added `Actor.request_instruments(...)` method
- Added `Order.would_reduce_only(...)` method
- Extended instrument(s) Req/Res handling for `DataClient` and `Actor

### Fixes
- Fixed memory management for Rust backing structs (now being properly freed)

---

# NautilusTrader 1.157.0 Beta

Released on 24th October 2022 (UTC).

### Breaking Changes
- None

### Enhancements
- Added experimental local order emulation for all order types (except `MARKET` and `MARKET_TO_LIMIT`) see docs
- Added `min_latency`, `max_latency` and `avg_latency` to `HttpClient` base class

### Fixes
- Fixed Binance Spot `display_qty` for iceberg orders, thanks @JackMa
- Fixed Binance HTTP client error logging

---

# NautilusTrader 1.156.0 Beta

Released on 19th October 2022 (UTC).

This will be the final release with support for Python 3.8.

### Breaking Changes
- Added `OrderSide.NONE` enum variant
- Added `PositionSide.NO_POSITION_SIDE` enum variant
- Changed order of `TriggerType` enum variants
- Renamed `AggressorSide.UNKNOWN` to `AggressorSide.NONE` (for consistency with other enums)
- Renamed `Order.type` to `Order.order_type` (reduces ambiguity and aligns with Rust struct field)
- Renamed `OrderInitialized.type` to `OrderInitialized.order_type` reduces ambiguity)
- Renamed `Bar.type` to `Bar.bar_type` (reduces ambiguity and aligns with Rust struct field)
- Removed redundant `check_position_exists` flag
- Removed `hyperopt` as considered unmaintained and there are better options
- Existing pickled data for `QuoteTick` is now **invalid** (change to schema for correctness)
- Existing catalog data for `OrderInitialized` is now **invalid** (change to schema for emulation)

### Enhancements
- Added configurable automated in-flight order status checks
- Added order `side` filter to numerous cache order methods
- Added position `side` filter to numerous cache position methods
- Added optional `order_side` to `cancel_all_orders` strategy method
- Added optional `position_side` to `close_all_positions` strategy method
- Added support for Binance Spot second bars
- Added `RelativeVolatilityIndex` indicator, thanks @graceyangfan
- Extracted `OrderMatchingEngine` from `SimulatedExchange` with refinements
- Extracted `MatchingCore` from `OrderMatchingEngine`
- Improved HTTP error handling and client logging (messages now contain reason)

### Fixes
- Fixed price and size precision validation for `QuoteTick` from raw values
- Fixed IB adapter data parsing for decimal precision
- Fixed HTTP error handling and releasing of response coroutines, thanks @JackMa
- Fixed `Position` calculations and account for when any base currency == commission currency, thanks @JackMa

---

# NautilusTrader 1.155.0 Beta

Released on September 15th 2022 (UTC).

This is an early release to address some parsing bugs in the FTX adapter.

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed parsing bug for FTX futures
- Fixed parsing bug for FTX `Bar`

---

# NautilusTrader 1.154.0 Beta

Released on September 14th 2022 (UTC).

### Breaking Changes
- Changed `ExecEngineConfig` `allow_cash_positions` default to `True` (more typical use case)
- Removed `check` parameter from `Bar` (always checked for simplicity)

### Enhancements
- Added `MARKET_TO_LIMIT` order implementation for `SimulatedExchange`
- Make strategy `order_id_tag` truly optional and auto incrementing
- Added PsychologicalLine indicator, thanks @graceyangfan
- Added initial Rust parquet integration, thanks @twitu and @ghill2
- Added validation for setting leverages on `CASH` accounts
- De-cythonized live data and execution client base classes for usability

### Fixes
- Fixed limit order `IOC` and `FOK` behavior, thanks @limx0 for identifying
- Fixed FTX `CryptoFuture` instrument parsing, thanks @limx0
- Fixed missing imports in data catalog example notebook, thanks @gaugau3000
- Fixed order update behavior, affected orders:
  - `LIMIT_IF_TOUCHED`
  - `MARKET_IF_TOUCHED`
  - `MARKET_TO_LIMIT`
  - `STOP_LIMIT`

---

# NautilusTrader 1.153.0 Beta

Released on September 6th 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Added trigger orders for FTX adapter
- Improved `BinanceBar` to handle enormous quote volumes
- Improved robustness of instrument parsing for Binance and FTX adapters
- Improved robustness of WebSocket message handling for Binance and FTX adapters
- Added `override_usd` option for FTX adapter
- Added `log_warnings` config option for Binance and FTX instrument providers
- Added `TRD_GRP_005` enum variant for Binance spot permissions

### Fixes
- Fixed bar aggregator partial bar handling
- Fixed `CurrencyType` variants in Rust
- Fixed missing `encoding` in Catalog parsing method, thanks @limx0 and @aviatorBeijing

---

# NautilusTrader 1.152.0 Beta

Released on September 1st 2022 (UTC).

### Breaking Changes
- Renamed `offset_type` to `trailing_offset_type`
- Renamed `is_frozen_account` to `frozen_account`
- Removed `bar_execution` from config API (implicitly turned on with bars currently)

### Enhancements
- Added `TRAILING_STOP_MARKET` order implementation for `SimulatedExchange`
- Added `TRAILING_STOP_LIMIT` order implementation for `SimulatedExchange`
- Added all simulated exchange options to `BacktestVenueConfig`

### Fixes
- Fixed creation and caching of order book on subscribing to deltas, thanks @limx0
- Fixed use of `LoopTimer` in live clock for trading node, thanks @sidnvy
- Fixed order cancels for IB adapter, thanks @limx0

---

# NautilusTrader 1.151.0 Beta

Released on August 22nd 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Added `on_historical_data` method with wiring for functionality
- Added 'unthrottled' 0ms order book updates for Binance Futures
- Improved robustness of `WebSocketClient` base during reconnects

### Fixes
- Fixed sdist includes for Rust Cargo files
- Fixed `LatencyModel` integer overflows, thanks @limx0
- Fixed parsing of Binance Futures `FUNDING_FEE` updates
- Fixed `asyncio.tasks.gather` for Python 3.10+

---

# NautilusTrader 1.150.0 Beta

Released on August 15th 2022 (UTC).

### Breaking Changes
- `BacktestEngine` now required venues to be added prior to instruments
- `BacktestEngine` now requires instruments to be added prior to data
- Renamed `Ladder.reverse` to `Ladder.is_reversed`
- Portfolio performance now displays commissions as a negative

### Enhancements
- Added initial backtest config validation for instrument vs venue
- Added initial sandbox execution client
- Added leverage options for `BacktestVenueConfig`, thanks @miller-moore
- Allow `Trader` to run without strategies loaded
- Integrated core Rust clock and timer
- De-cythonize `InstrumentProvider` base class

### Fixes
- Fixed double counting of commissions for single-currency and multi-currency accounts #657

---

# NautilusTrader 1.149.0 Beta

Released on 27th June 2022 (UTC).

### Breaking Changes
- Schema change for `Instrument.info` for `ParquetDataCatalog`

### Enhancements
- Added `DirectionalMovementIndicator` indicator, thanks @graceyangfan
- Added `KlingerVolumeOscillator` indicator, thanks @graceyangfan
- Added `clientId` and `start_gateway` for IB config, thanks @niks199

### Fixes
- Fixed macOS ARM64 build
- Fixed Binance testnet URL
- Fixed IB contract ID dict, thanks @niks199
- Fixed IB `InstrumentProvider` #685, thanks @limx0
- Fixed IB orderbook snapshots L1 value assertion #712 , thanks @limx0

---

# NautilusTrader 1.148.0 Beta

Released on 30th June 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Ported core bar objects to Rust thanks @ghill2
- Improved core `unix_nanos_to_iso8601` performance by 30% thanks @ghill2
- Added `DataCatalog` interface for `ParquetDataCatalog` thanks @jordanparker6
- Added `AroonOscillator` indicator thanks @graceyangfan
- Added `ArcherMovingAveragesTrends` indicator thanks @graceyangfan
- Added `DoubleExponentialMovingAverage` indicator thanks @graceyangfan
- Added `WilderMovingAverage` indicator thanks @graceyangfan
- Added `ChandeMomentumOscillator` indicator thanks @graceyangfan
- Added `VerticalHorizontalFilter` indicator thanks @graceyangfan
- Added `Bias` indicator thanks @graceyangfan

### Fixes
None

---

# NautilusTrader 1.147.1 Beta

Released on 6th June 2022 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed incorrect backtest log timestamps (was using actual time)
- Fixed formatting of timestamps for nanoseconds zulu as per RFC3339

---

# NautilusTrader 1.147.0 Beta

Released on 4th June 2022 (UTC).

### Breaking Changes
None

### Enhancements
- Improved error handling for invalid state triggers
- Improved component state transition behavior and logging
- Improved `TradingNode` disposal flow
- Implemented core monotonic clock
- Implemented logging in Rust
- Added `CommodityChannelIndex` indicator thanks @graceyangfan

### Fixes
None

---

# NautilusTrader 1.146.0 Beta

Released on 22nd May 2022 (UTC).

### Breaking Changes
- `AccountId` constructor now takes single value string
- Removed redundant `UUIDFactory` and all associated backing fields and calls
- Removed `ClientOrderLinkId` (not in use)

### Enhancements
- Refinements and improvements to Rust core

### Fixes
- Fixed pre-trade notional risk checks incorrectly applied to `MARGIN` accounts
- Fixed `net_qty` in `PositionStatusReport` thanks to @sidnvy
- Fixed `LinearRegression` indicator thanks to @graceyangfan

---

# NautilusTrader 1.145.0 Beta

Released on 15th May 2022 (UTC).

This is an early release due to the build error in the sdist for `1.144.0`.
The error is due to the `nautilus_core` Rust source not being included in the sdist package.

### Breaking Changes
- All raw order constructors now take `expire_time_ns` int64 rather than a datetime
- All order serializations due to `expire_time_ns` option handling
- `PortfolioAnalyzer` moved from `Trader` to `Portfolio`

### Enhancements
- `PortfolioAnalyzer` now available to strategies via `self.portfolio.analyzer`

### Fixes
None

---

# NautilusTrader 1.144.0 Beta

Released on 10th May 2022 (UTC).

### Breaking Changes
- Removed `BacktestEngine.add_ticks()` as redundant with `.add_data()`
- Removed `BacktestEngine.add_bars()` as redundant with `.add_data()`
- Removed `BacktestEngine.add_generic_data()` as redundant with `.add_data()`
- Removed `BacktestEngine.add_order_book_data()` as redundant with `.add_data()`
- Renamed `Position.from_order` to `Position.opening_order_id`
- Renamed `StreamingPersistence` to `StreamingFeatherWriter`
- Renamed `PersistenceConfig` to `StreamingConfig`
- Renamed `PersistenceConfig.flush_interval` to `flush_interval_ms`

### Enhancements
- Added `Actor.publish_signal` for generic dynamic signal data
- Added `WEEK` and `MONTH` bar aggregation options
- Added `Position.closing_order_id` property
- Added `tags` parameter to `Strategy.submit_order`
- Added optional `check_position_exists` flag to `Strategy.submit_order`
- Eliminated all use of `unsafe` Rust and C null-terminated byte strings
- The `bypass_logging` config option will also now bypass the `BacktestEngine` logger

### Fixes
- Fixed behavior of `IOC` and `FOK` time in force instructions
- Fixed Binance bar resolution parsing

---

# NautilusTrader 1.143.0 Beta

Released on 21st April 2022 (UTC).

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fixed segfault for `CashAccount.calculate_balance_locked` with no base currency
- Various FeatherWriter fixes

---

# NautilusTrader 1.142.0 Beta

Released on 17th April 2022 (UTC).

### Breaking Changes
- `BacktestNode` now requires configs at initialization
- Removed `run_configs` parameter from `BacktestNode.run()` method
- Removed `return_engine` flag
- Renamed `TradingStrategy` to `Strategy`
- Renamed `TradingStrategyConfig` to `StrategyConfig`
- Changes to configuration object import paths
- Removed redundant `realized_points` concept from `Position`

### Enhancements
- Added `BacktestNode.get_engines()` method
- Added `BacktestNode.get_engine(run_config_id)` method
- Added `Actor.request_instrument()` method (also applies to `Strategy`)
- Added `Cache.snapshot_position()` method
- All configuration objects can now be imported directly from `nautilus_trader.config`
- Execution engine now takes snapshots of closed netted positions
- Performance statistics now based on total positions and snapshots
- Added Binance Spot/Margin external order handling
- Added support for millisecond bar aggregation
- Added configurable `debug` mode for engines (with extra debug logging)
- Improved annualized portfolio statistics with configurable period

### Fixes
None

---

# NautilusTrader 1.141.0 Beta

Released on 4th April 2022 (UTC).

### Breaking Changes
- Renamed `BacktestNode.run_sync()` to `BacktestNode.run()`
- Renamed `flatten_position()` to `close_position()`
- Renamed `flatten_all_positions()` to `close_all_positions()`
- Renamed `Order.flatten_side()` to `Order.closing_side()`
- Renamed `TradingNodeConfig` `check_residuals_delay` to `timeout_post_stop`
- The `SimulatedExchange` will now 'receive' market data prior to the `DataEngine`
  (note that this did not affect any test)
- Tightened requirement for `DataType` types to be subclasses of `Data`
- `CacheDatabaseConfig.type` now defaults to `in-memory`
- `NAUTILUS_CATALOG` env var changed to `NAUTILUS_PATH`
- `DataCatalog` root path now located under `$OLD_PATH/catalog/` from the Nautilus path
- `hiredis` and `redis` are now optional extras as 'redis'
- `hyperopt` is now an optional extra as 'hyperopt'

### Enhancements
- Unify `NautilusKernel` across backtest and live systems
- Improved configuration by grouping into `config` subpackage
- Improved configuration objects and flows
- Numerous improvements to the Binance Spot/Margin and Futures integration
- Added Docker image builds and GH packages
- Added `BinanceFuturesMarkPriceUpdate` type and data stream
- Added generic `subscribe` and `unsubscribe` to template
- Added Binance Futures COIN_M testnet
- The clarity of various error messages was improved

### Fixes
- Fixed multiple instruments in `DataCatalog` (#554), (#560) by @limx0
- Fixed timestamp ordering streaming from `DataCatalog` (#561) by @limx0
- Fixed `CSVReader` (#563) by @limx0
- Fixed slow subscribers to the Binance WebSocket streams
- Fixed configuration of `base_currency` for backtests
- Fixed importable strategy configs (previously not returning correct class)
- Fixed `fully_qualified_name()` format

---

# NautilusTrader 1.140.0 Beta

## Release Notes

Released on 13th March 2022 (UTC).

This is a patch release which fixes a moderate severity security vulnerability in
pillow < 9.0.1:

    If the path to the temporary directory on Linux or macOS contained a space,
    this would break removal of the temporary image file after im.show() (and related actions),
    and potentially remove an unrelated file. This been present since PIL.

This release upgrades to pillow 9.0.1.

Note the minor version was incremented in error.

---

# NautilusTrader 1.139.0 Beta

## Release Notes

Released on 11th March 2022 (UTC).

### Breaking Changes
- Renamed `CurrencySpot` to `CurrencyPair`
- Renamed `PerformanceAnalyzer` to `PortfolioAnalyzer`
- Renamed `BacktestDataConfig.data_cls_path` to `data_cls`
- Renamed `BinanceTicker` to `BinanceSpotTicker`
- Renamed `BinanceSpotExecutionClient` to `BinanceExecutionClient`

### Enhancements
- Added initial **(beta)** Binance Futures adapter implementation
- Added initial **(beta)** Interactive Brokers adapter implementation
- Added custom portfolio statistics
- Added `CryptoFuture` instrument
- Added `OrderType.MARKET_TO_LIMIT`
- Added `OrderType.MARKET_IF_TOUCHED`
- Added `OrderType.LIMIT_IF_TOUCHED`
- Added `MarketToLimitOrder` order type
- Added `MarketIfTouchedOrder` order type
- Added `LimitIfTouchedOrder` order type
- Added `Order.has_price` property (convenience)
- Added `Order.has_trigger_price` property (convenience)
- Added `msg` parameter to `LoggerAdapter.exception()`
- Added WebSocket `log_send` and `log_recv` config options
- Added WebSocket `auto_ping_interval` (seconds) config option
- Replaced `msgpack` with `msgspec` (faster drop in replacement https://github.com/jcrist/msgspec)
- Improved exception messages by providing helpful context
- Improved `BacktestDataConfig` API: now takes either a type of `Data` _or_ a fully qualified path string

### Fixes
- Fixed FTX execution WebSocket 'ping strategy'
- Fixed non-deterministic config dask tokenization

---

# NautilusTrader 1.138.0 Beta

## Release Notes

Released on 15th February 2022 (UTC).

**This release contains numerous method, parameter and property name changes**

For consistency and standardization with other protocols, the `ExecutionId` type
has been renamed to `TradeId` as they express the same concept with a more
standardized terminology. In the interests of enforcing correctness and
safety this type is now utilized for the `TradeTick.trade_id`.

### Breaking Changes
- Renamed `working` orders to `open` orders including all associated methods and params
- Renamed `completed` orders to `closed` orders including all associated methods and params
- Removed `active` order concept (often confused with `open`)
- Renamed `trigger` to `trigger_price`
- Renamed `StopMarketOrder.price` to `StopMarketOrder.trigger_price`
- Renamed all params related to a `StopMarketOrders` `price` to `trigger_price`
- Renamed `ExecutionId` to `TradeId`
- Renamed `execution_id` to `trade_id`
- Renamed `Order.trade_id` to `Order.last_trade_id` (for clarity)
- Renamed other variations and references of 'execution ID' to 'trade ID'
- Renamed `contingency` to `contingency_type`

### Enhancements
- Introduced the `TradeId` type to enforce `trade_id` typing
- Improve handling of unleveraged cash asset positions including Crypto and Fiat spot currency instruments
- Added `ExecEngineConfig` config option `allow_cash_positions` (`False` by default)
- Added `TrailingOffsetType` enum
- Added `TrailingStopMarketOrder`
- Added `TrailingStopLimitOrder`
- Added trailing order factory methods
- Added `trigger_type` parameter to stop orders
- Added `TriggerType` enum
- Large refactoring of order base and impl classes
- Overhaul of execution reports
- Overhaul of execution state reconciliation

### Fixes
- Fixed WebSocket base reconnect handling

---

# NautilusTrader 1.137.1 Beta

## Release Notes

Released on 15th January 2022 (UTC).

This is a patch release which fixes moderate to high severity security vulnerabilities in
`pillow < 9.0.0`:
- PIL.ImageMath.eval allows evaluation of arbitrary expressions, such as ones that use the Python exec method
- path_getbbox in path.c has a buffer over-read during initialization of ImagePath.Path
- path_getbbox in path.c improperly initializes ImagePath.Path

This release upgrades to `pillow 9.0.0`.

---

# NautilusTrader 1.137.0 Beta

## Release Notes

Released on 12th January 2022 (UTC).

### Breaking Changes
- Removed redundant `currency` parameter from `AccountBalance`
- Renamed `local_symbol` to `native_symbol`
- Removed the `VenueType` enum and `venue_type` parameter in favour of a `routing` bool flag
- Removed `account_id` parameter from execution client factories and constructors
- Changed venue generated IDs (order, execution, position) which now begin with the venue ID

### Enhancements
- Added FTX integration for testing
- Added FTX US configuration option
- Added Binance US configuration option
- Added `MarginBalance` object to assist with margin account functionality

### Fixes
- Fixed parsing of `BarType` with symbols including hyphens `-`
- Fixed `BinanceSpotTicker` `__repr__` (was missing whitespace after a comma)
- Fixed `DataEngine` requests for historical `TradeTick`
- Fixed `DataEngine` `_handle_data_response` typing of `data` to `object`

---

# NautilusTrader 1.136.0 Beta

## Release Notes

Released on 29th December 2021.

### Breaking Changes
- Changed `subscribe_data(...)` method (`client_id` now optional)
- Changed `unsubscribe_data(...)` method (`client_id` now optional)
- Changed `publish_data(...)` method (added `data_type`)
- Renamed `MessageBus.subscriptions` method parameter to `pattern`
- Renamed `MessageBus.has_subscribers` method parameter to `pattern`
- Removed `subscribe_strategy_data(...)` method
- Removed `unsubscribe_strategy_data(...)` method
- Removed `publish_strategy_data(...)` method
- Renamed `CryptoSwap` to `CryptoPerpetual`

### Enhancements
- Can now modify or cancel in-flight orders live and backtest
- Updated `CancelOrder` to allow None `venue_order_id`
- Updated `ModifyOrder` to allow None `venue_order_id`
- Updated `OrderPendingUpdate` to allow None `venue_order_id`
- Updated `OrderPendingCancel` to allow None `venue_order_id`
- Updated `OrderCancelRejected` to allow None `venue_order_id`
- Updated `OrderModifyRejected` to allow None `venue_order_id`
- Added `DataType.topic` string for improved message bus handling

### Fixes
- Implemented comparisons for `DataType`, `BarSpecification` and `BarType`
- Fixed `QuoteTickDataWrangler.process_bar_data` with `random_seed`

---

# NautilusTrader 1.135.0 Beta

## Release Notes

Released on 13th December 2021.

### Breaking Changes
- Renamed `match_id` to `trade_id`

### Enhancements
- Added bars method to `DataCatalog`
- Improved parsing of Binance historical bars data
- Added `CancelAllOrders` command
- Added bulk cancel capability to Binance integration
- Added bulk cancel capability to Betfair integration

### Fixes
- Fixed handling of `cpu_freq` call in logging for ARM architecture
- Fixed market order fill edge case for bar data
- Fixed handling of `GenericData` in backtests

---

# NautilusTrader 1.134.0 Beta

## Release Notes

Released on 22nd November 2021.

### Breaking Changes
- Changed `hidden` order option to `display_qty` to support iceberg orders
- Renamed `Trader.component_ids()` to `Trader.actor_ids()`
- Renamed `Trader.component_states()` to `Trader.actor_states()`
- Renamed `Trader.add_component()` to `Trader.add_actor()`
- Renamed `Trader.add_components()` to `Trader.add_actors()`
- Renamed `Trader.clear_components()` to `Trader.clear_actors()`

### Enhancements
- Added initial implementation of Binance SPOT integration (beta stage testing)
- Added support for display quantity/iceberg orders

### Fixes
- Fixed `Actor` clock time advancement in backtest engine

---

# NautilusTrader 1.133.0 Beta

## Release Notes

Released on 8th November 2021.

### Breaking Changes
None

### Enhancements
- Added `LatencyModel` for simulated exchange
- Added `last_update_id` to order books
- Added `update_id` to order book data
- Added `depth` parameter when subscribing to order book deltas
- Added `Clock.timestamp_ms()`
- Added `TestDataProvider` and consolidate test data
- Added orjson default serializer for arrow
- Reorganized example strategies and launch scripts

### Fixes
- Fixed logic for partial fills in backtests
- Various Betfair integration fixes
- Various `BacktestNode` fixes

---

# NautilusTrader 1.132.0 Beta

## Release Notes

Released on 24th October 2021.

### Breaking Changes
- `Actor` constructor now takes `ActorConfig`

### Enhancements
- Added `ActorConfig`
- Added `ImportableActorConfig`
- Added `ActorFactory`
- Added `actors` to `BacktestRunConfig`
- Improved network base classes
- Refine `InstrumentProvider`

### Fixes
- Fixed persistence config for `BacktestNode`
- Various Betfair integration fixes

---

# NautilusTrader 1.131.0 Beta

## Release Notes

Released on 10th October 2021.

### Breaking Changes
- Renamed `nanos_to_unix_dt` to `unix_nanos_to_dt` (more accurate name)
- Changed `Clock.set_time_alert(...)` method signature
- Changed `Clock.set_timer(...)` method signature
- Removed `pd.Timestamp` from `TimeEvent`

### Enhancements
- `OrderList` submission and OTO, OCO contingencies now operational
- Added `Cache.orders_for_position(...)` method
- Added `Cache.position_for_order(...)` method
- Added `SimulatedExchange.get_working_bid_orders(...)` method
- Added `SimulatedExchange.get_working_ask_orders(...)` method
- Added optional `run_config_id` for backtest runs
- Added `BacktestResult` object
- Added `Clock.set_time_alert_ns(...)` method
- Added `Clock.set_timer_ns(...)` method
- Added `fill_limit_at_price` simulated exchange option
- Added `fill_stop_at_price` simulated exchange option
- Improve timer and time event efficiency

### Fixes
- Fixed `OrderUpdated` leaves quantity calculation
- Fixed contingency order logic at the exchange
- Fixed indexing of orders for a position in the cache
- Fixed flip logic for zero-sized positions (not a flip)

---

# NautilusTrader 1.130.0 Beta

## Release Notes

Released on 26th September 2021.

### Breaking Changes
- `BacktestEngine.run` method signature change
- Renamed `BookLevel` to `BookType`
- Renamed `FillModel` params

### Enhancements
- Added streaming backtest machinery.
- Added `quantstats` (removed `empyrical`)
- Added `BacktestEngine.run_streaming()`
- Added `BacktestEngine.end_streaming()`
- Added `Portfolio.balances_locked(venue)`
- Improved `DataCatalog` functionality
- Improved logging for `BacktestEngine`
- Improved parquet serialization and machinery

### Fixes
- Fixed `SimulatedExchange` message processing
- Fixed `BacktestEngine` event ordering in main loop
- Fixed locked balance calculation for `CASH` accounts
- Fixed fill dynamics for `reduce-only` orders
- Fixed `PositionId` handling for `HEDGING` OMS exchanges
- Fixed parquet `Instrument` serialization
- Fixed `CASH` account PnL calculations with base currency

---

# NautilusTrader 1.129.0 Beta

## Release Notes

Released on 12th September 2021.

### Breaking Changes
- Removed CCXT adapter (#428)
- Backtest configuration changes
- Renamed `UpdateOrder` to `ModifyOrder` (terminology standardization)
- Renamed `DeltaType` to `BookAction` (terminology standardization)

### Enhancements
- Added `BacktestNode`
- Added `BookIntegrityError` with improved integrity checks for order books
- Added order custom user tags
- Added `Actor.register_warning_event` (also applicable to `TradingStrategy`)
- Added `Actor.deregister_warning_event` (also applicable to `TradingStrategy`)
- Added `ContingencyType` enum (for contingent orders in an `OrderList`)
- All order types can now be `reduce_only` (#437)
- Refined backtest configuration options
- Improved efficiency of `UUID4` using the Rust `fastuuid` Python bindings

### Fixes
- Fixed Redis loss of precision for `int64_t` nanosecond timestamps (#363)
- Fixed behavior of `reduce_only` orders for both submission and filling (#437)
- Fixed PnL calculation for `CASH` accounts when commission negative (#436), thanks for reporting @imcu

---

# NautilusTrader 1.128.0 Beta - Release Notes

Released on 30th August 2021.

This release continues the focus on the core system, with upgrades and cleanups
to the component base class. The concept of an `active` order has been introduced,
which is an order whose state can change (is not a `completed` order).

### Breaking Changes
- All configuration due `pydantic` upgrade
- Throttling config now takes string e.g. "100/00:00:01" which is 100 / second
- Renamed `DataProducerFacade` to `DataProducer`
- Renamed `fill.side` to `fill.order_side` (clarity and standardization)
- Renamed `fill.type` to `fill.order_type` (clarity and standardization)

### Enhancements
- Added serializable configuration classes leveraging `pydantic`
- Improved adding bar data to `BacktestEngine`
- Added `BacktestEngine.add_bar_objects()`
- Added `BacktestEngine.add_bars_as_ticks()`
- Added order `active` concept, with `order.is_active` and cache methods
- Added `ComponentStateChanged` event
- Added `Component.degrade()` and `Component.fault()` command methods
- Added `Component.on_degrade()` and `Component.on_fault()` handler methods
- Added `ComponentState.PRE_INITIALIZED`
- Added `ComponentState.DEGRADING`
- Added `ComponentState.DEGRADED`
- Added `ComponentState.FAULTING`
- Added `ComponentState.FAULTED`
- Added `ComponentTrigger.INITIALIZE`
- Added `ComponentTrigger.DEGRADE`
- Added `ComponentTrigger.DEGRADED`
- Added `ComponentTrigger.FAULT`
- Added `ComponentTrigger.FAULTED`
- Wired up `Ticker` data type

### Fixes
- `DataEngine.subscribed_bars()` now reports internally aggregated bars also.

---

# NautilusTrader 1.127.0 Beta

## Release Notes

Released on 17th August 2021.

This release has again focused on core areas of the platform, including a
significant overhaul of accounting and portfolio components. The wiring between
the `DataEngine` and `DataClient`(s) has also received attention, and should now
exhibit correct subscription mechanics.

The Betfair adapter has been completely re-written, providing various fixes and
enhancements, increased performance, and full async support.

There has also been some further renaming to continue to align the platform
as closely as possible with established terminology in the domain.

### Breaking Changes
- Moved margin calculation methods from `Instrument` to `Account`
- Removed redundant `Portfolio.register_account`
- Renamed `OrderState` to `OrderStatus`
- Renamed `Order.state` to `Order.status`
- Renamed `msgbus.message_bus` to `msgbus.bus`

### Enhancements
- Betfair adapter re-write
- Extracted `accounting` subpackage
- Extracted `portfolio` subpackage
- Subclassed `Account` with `CashAccount` and `MarginAccount`
- Added `AccountsManager`
- Added `AccountFactory`
- Moved registration of custom account classes to `AccountFactory`
- Moved registration of calculated account to `AccountFactory`
- Added registration of OMS type per trading strategy
- Added `ExecutionClient.create_account` for custom account classes
- Separate `PortfolioFacade` from `Portfolio`

### Fixes
- Data subscription handling in `DataEngine`
- `Cash` accounts no longer generate spurious margins
- Fix `TimeBarAggregator._stored_close_ns` property name

---

# NautilusTrader 1.126.1 Beta

## Release Notes

Released on 3rd August 2021.

This is a patch release which fixes a bug involving `NotImplementedError`
exception handling when subscribing to order book deltas when not supported by
a client. This bug affected CCXT order book subscriptions.

### Breaking Changes
None

### Enhancements
None

### Fixes
- Fix `DataEngine` order book subscription handling

---

# NautilusTrader 1.126.0 Beta

## Release Notes

Released on 2nd August 2021.

This release sees the completion of the initial implementation of the
`MessageBus`, with data now being handled by Pub/Sub patterns, along with the
additions of point-to-point and Req/Rep messaging functionality.

An `Actor` base class has been abstracted from `TradingStrategy` which allows
custom components to be added to a `Trader` which aren't necessarily trading
strategies, opening up further possibilities for extending NautilusTrader with
custom functionality.

For the sake of simplicity and to favour more idiomatic Python, the null object
pattern is no longer utilized for handling identifiers. This has removed a layer
of 'logical indirection' in certain parts of the codebase, and allows for simpler
code.

An order is now considered 'in-flight' if it is actively pending a state
transition i.e. in the `SUBMITTED`,`PENDING_UPDATE` or `PENDING_CANCEL` states.

It is now a well established convention that all integer based timestamps are
expressed in UNIX nanoseconds, therefore the `_ns` postfix has now been dropped.
For clarity - time periods/intervals/objects where the units may not be obvious
have retained the `_ns` postfix.

The opportunity was identified to unify the parameter naming for the concept
of object instantiation by renaming `timestamp_ns` and `ts_recv_ns` to `ts_init`.
Along the same lines, the timestamps for both event and data occurrence have
been standardized to `ts_event`.

It is acknowledged that the frequent name changes and modifications to core
concepts may be frustrating, however whilst still in a beta phase - we're taking
the opportunity to lay a solid foundation for this project to continue to growth
in the years ahead.

### Breaking Changes
- Renamed `timestamp_ns` to `ts_init`
- Renamed `ts_recv_ns` to `ts_event`
- Renamed various event timestamp parameters to `ts_event`
- Removed null object methods on identifiers

### Enhancements
- Added `Actor` component base class
- Added `MessageBus.register()`
- Added `MessageBus.send()`
- Added `MessageBus.request()`
- Added `MessageBus.response()`
- Added `Trader.add_component()`
- Added `Trader.add_components()`
- Added `Trader.add_log_sink()`

### Fixes
- Various Betfair adapter patches and fixes
- `ExecutionEngine` position flip logic in certain edge cases

---

# NautilusTrader 1.125.0 Beta

## Release Notes

Released on 18th July 2021.

This release introduces a major re-architecture of the internal messaging system.
A common message bus has been implemented which now handles all events via a
Pub/Sub messaging pattern. The next release will see all data being handled by
the message bus, see the related issue for further details on this enhancement.

Another notable feature is the introduction of the order 'in-flight' concept,
which is a submitted order which has not yet been acknowledged by the
trading venue. Several properties on `Order`, and methods on `Cache`, now exist
to support this.

The `Throttler` has been refactored and optimized further. There has also been
extensive reorganization of the model sub-package, standardization of identifiers
on events, along with numerous 'under the hood' cleanups and two bug fixes.

### Breaking Changes
- Renamed `MessageType` enum to `MessageCategory`
- Renamed `fill.order_side` to `fill.side`
- Renamed `fill.order_type` to `fill.type`
- All `Event` serialization due to domain refactorings

### Enhancements
- Added `MessageBus` class
- Added `TraderId` to `Order` and `Position`
- Added `OrderType` to OrderFilled
- Added unrealized PnL to position events
- Added order in-flight concept to `Order` and `Cache`
- Improved efficiency of `Throttler`
- Standardized events `str` and `repr`
- Standardized commands `str` and `repr`
- Standardized identifiers on events and objects
- Improved `Account` `str` and `repr`
- Using `orjson` over `json` for efficiency
- Removed redundant `BypassCacheDatabase`
- Introduced `mypy` to the codebase

### Fixes
- Fixed backtest log timestamping
- Fixed backtest duplicate initial account event

---

# NautilusTrader 1.124.0 Beta

## Release Notes

Released on 6th July 2021.

This release sees the expansion of pre-trade risk check options (see
`RiskEngine` class documentation). There has also been extensive 'under the
hood' code cleanup and consolidation.

### Breaking Changes
- Renamed `Position.opened_timestamp_ns` to `ts_opened_ns`
- Renamed `Position.closed_timestamp_ns` to `ts_closed_ns`
- Renamed `Position.open_duration_ns` to `duration_ns`
- Renamed Loggers `bypass_logging` to `bypass`
- Refactored `PositionEvent` types

### Enhancements
- Added pre-trade risk checks to `RiskEngine` iteration 2
- Improve `Throttler` functionality and performance
- Removed redundant `OrderInvalid` state and associated code
- Improve analysis reports

### Fixes
- PnL calculations for `CASH` account types
- Various event serializations

---

# NautilusTrader 1.123.0 Beta

## Release Notes

Released on 20th June 2021.

A major feature of this release is a complete re-design of serialization for the
platform, along with initial support for the [Parquet](https://parquet.apache.org/) format.
The MessagePack serialization functionality has been refined and retained.

In the interests of explicitness there is now a convention that timestamps are
named either `timestamp_ns`, or prepended with `ts`. Timestamps which are
represented with an `int64` are always in nanosecond resolution, and appended
with `_ns` accordingly.

Initial scaffolding for new backtest data tooling has been added.

### Breaking Changes
- Renamed `OrderState.PENDING_REPLACE` to `OrderState.PENDING_UPDATE`
- Renamed `timestamp_origin_ns` to `ts_event_ns`
- Renamed `timestamp_ns` for data to `ts_recv_ns`
- Renamed `updated_ns` to `ts_updated_ns`
- Renamed `submitted_ns` to `ts_submitted_ns`
- Renamed `rejected_ns` to `ts_rejected_ns`
- Renamed `accepted_ns` to `ts_accepted_ns`
- Renamed `pending_ns` to `ts_pending_ns`
- Renamed `canceled_ns` to `ts_canceled_ns`
- Renamed `triggered_ns` to `ts_triggered_ns`
- Renamed `expired_ns` to `ts_expired_ns`
- Renamed `execution_ns` to `ts_filled_ns`
- Renamed `OrderBookLevel` to `BookLevel`
- Renamed `Order.volume` to `Order.size`

### Enhancements
- Adapter dependencies are now optional extras at installation
- Added arrow/parquet serialization
- Added object `to_dict()` and `from_dict()` methods
- Added `Order.is_pending_update`
- Added `Order.is_pending_cancel`
- Added `run_analysis` config option for `BacktestEngine`
- Removed `TradeMatchId` in favour of bare string
- Removed redundant conversion to `pd.Timestamp` when checking timestamps
- Removed redundant data `to_serializable_str` methods
- Removed redundant data `from_serializable_str` methods
- Removed redundant `__ne__` implementations
- Removed redundant `MsgPackSerializer` cruft
- Removed redundant `ObjectCache` and `IdentifierCache`
- Removed redundant string constants

### Fixes
- Fixed millis to nanos in `CCXTExecutionClient`
- Added missing trigger to `UpdateOrder` handling
- Removed all `import *`

---

# NautilusTrader 1.122.0 Beta

## Release Notes

Released on 6th June 2021.

This release includes numerous breaking changes with a view to enhancing the core
functionality and API of the platform. The data and execution caches have been
unified for simplicity. There have also been large changes to the accounting
functionality, with 'hooks' added in preparation for accurate calculation and
handling of margins.

### Breaking Changes
- Renamed `Account.balance()` to `Account.balance_total()`
- Consolidated`TradingStrategy.data` into `TradingStrategy.cache`
- Consolidated `TradingStrategy.execution` into `TradingStrategy.cache`
- Moved `redis` subpackage into `infrastructure`
- Moved some accounting methods back to `Instrument`
- Removed `Instrument.market_value()`
- Renamed `Portfolio.market_values()` to `Portfolio.net_exposures()`
- Renamed `Portfolio.market_value()` to `Portfolio.net_exposure()`
- Renamed `InMemoryExecutionDatabase` to `BypassCacheDatabase`
- Renamed `Position.relative_qty` to `Position.net_qty`
- Renamed `default_currency` to `base_currency`
- Removed `cost_currency` property from `Instrument`

### Enhancements
- `ExecutionClient` now has the option of calculating account state
- Unified data and execution caches into single `Cache`
- Improved configuration options and naming
- Simplified `Portfolio` component registration
- Simplified wiring of `Cache` into components
- Added `repr` to execution messages
- Added `AccountType` enum
- Added `cost_currency` to `Position`
- Added `get_cost_currency()` to `Instrument`
- Added `get_base_currency()` to `Instrument`

### Fixes
- Fixed `Order.is_working` for `PENDING_CANCEL` and `PENDING_REPLACE` states
- Fixed loss of precision for nanosecond timestamps in Redis
- Fixed state reconciliation when uninstantiated client

---

# NautilusTrader 1.121.0 Beta

## Release Notes

Released on 30th May 2021.

In this release there has been a major change to the use of inlines for method
signatures. From the Cython docs:
_"Note that class-level cdef functions are handled via a virtual function table
so the compiler won’t be able to inline them in almost all cases."_.
https://cython.readthedocs.io/en/latest/src/userguide/pyrex_differences.html?highlight=inline.

It has been found that adding `inline` to method signatures makes no difference
to the performance of the system - and so they have been removed to reduce
'noise' and simplify the codebase. Note that the use of `inline` for
module level functions will be passed to the C compiler with the expected
result of inlining the function.

### Breaking Changes
- `BacktestEngine.add_venue` added `venue_type` to method params
- `ExecutionClient` added `venue_type` to constructor params
- `TraderId` instantiation
- `StrategyId` instantiation
- `Instrument` serialization

### Enhancements
- `Portfolio` pending calculations if data not immediately available
- Added `instruments` subpackage with expanded class definitions
- Added `timestamp_origin_ns` timestamp when originally occurred
- Added `AccountState.is_reported` flagging if reported by exchange or calculated
- Simplified `TraderId` and `StrategyId` identifiers
- Improved `ExecutionEngine` order routing
- Improved `ExecutionEngine` client registration
- Added order routing configuration
- Added `VenueType` enum and parser
- Improved parameter typing for identifier generators
- Improved log formatting of `Money` and `Quantity` thousands commas

### Fixes
- CCXT `TICK_SIZE` precision mode - size precisions (BitMEX, FTX)
- State reconciliation (various bugs)

---

# NautilusTrader 1.120.0 Beta

## Release Notes

This release focuses on simplifications and enhancements of existing machinery

### Breaking Changes
- `Position` now requires an `Instrument` param
- `is_inverse` removed from `OrderFilled`
- `ClientId` removed from `TradingCommand` and subclasses
- `AccountId` removed from `TradingCommand` and subclasses
- `TradingCommand` serialization

### Enhancements
- Added `Instrument` methods to `ExecutionCache`
- Added `Venue` filter to cache queries
- Moved order validations into `RiskEngine`
- Refactored `RiskEngine`
- Removed routing type information from identifiers

### Fixes
None

---

# NautilusTrader 1.119.0 Beta

## Release Notes

This release applies another major refactoring to the value object API for
`BaseDecimal` and its subclasses `Price` and `Quantity`. Previously a precision
was not explicitly required when passing in a `decimal.Decimal` type which
sometimes resulted in unexpected behavior when a user passed in a decimal with
a very large precision (when wrapping a float with `decimal.Decimal`).

Convenience methods have been added to `Price` and `Quantity` where precision
is implicitly zero for ints, or implied in the number of digits after the '.'
point for strings. Convenience methods have also been added to `Instrument` to
assist the UX.

The serialization of `Money` has been improved with the inclusion of the
currency code in the string delimited by whitespace. This avoids an additional
field for the currency code.

`RiskEngine` has been rewired ahead of `ExecutionEngine` which clarifies areas
of responsibility and cleans up the registration sequence and allows a more
natural flow of command and event messages.

### Breaking Changes
- Serializations involving `Money`
- Changed usage of `Price` and `Quantity`
- Renamed `BypassExecutionDatabase` to `BypassCacheDatabase`

### Enhancements
- Rewired `RiskEngine` and `ExecutionEngine` sequence
- Added `Instrument` database operations
- Added `MsgPackInstrumentSerializer`
- Added `Price.from_str()`
- Added `Price.from_int()`
- Added `Quantity.zero()`
- Added `Quantity.from_str()`
- Added `Quantity.from_int()`
- Added `Instrument.make_price()`
- Added `Instrument.make_qty()`
- Improved serialization of `Money`

### Fixes
- Handling of precision for `decimal.Decimal` values passed to value objects

---

# NautilusTrader 1.118.0 Beta

## Release Notes

This release simplifies the backtesting workflow by removing the need for the
intermediate `BacktestDataContainer`. There has also been some simplifications
for `OrderFill` events, as well as additional order states and events.

### Breaking Changes
- Standardized all 'cancelled' references to 'canceled'.
- `SimulatedExchange` no longer generates `OrderAccepted` for `MarketOrder`
- Removed redundant `BacktestDataContainer`
- Removed redundant `OrderFilled.cum_qty`
- Removed redundant `OrderFilled.leaves_qty`
- `BacktestEngine` constructor simplified
- `BacktestMarketDataClient` no longer needs instruments
- Renamed `PortfolioAnalyzer.get_realized_pnls` to `.realized_pnls`

### Enhancements
- Re-engineered `BacktestEngine` to take data directly
- Added `OrderState.PENDING_CANCEL`
- Added `OrderState.PENDING_REPLACE`
- Added `OrderPendingUpdate` event
- Added `OrderPendingCancel` event
- Added `OrderFilled.is_buy` property (with corresponding `is_buy_c()` fast method)
- Added `OrderFilled.is_sell` property (with corresponding `is_sell_c()` fast method)
- Added `Position.is_opposite_side(OrderSide side)` convenience method
- Modified the `Order` FSM and event handling for the above
- Consolidated event generation into `ExecutionClient` base class
- Refactored `SimulatedExchange` for greater clarity

### Fixes
- `ExecutionCache` positions open queries
- Exchange accounting for exchange `OmsType.NETTING`
- Position flipping logic for exchange `OmsType.NETTING`
- Multi-currency account terminology
- Windows wheel packaging
- Windows path errors

---

# NautilusTrader 1.117.0 Beta

## Release Notes

The major thrust of this release is added support for order book data in
backtests. The `SimulatedExchange` now maintains order books of each instrument
and will accurately simulate market impact with L2/L3 data. For quote and trade
tick data a L1 order book is used as a proxy. A future release will include
improved fill modelling assumptions and customizations.

### Breaking Changes
- `OrderBook.create` now takes `Instrument` and `BookLevel`

### Enhancements
- `SimulatedExchange` now maintains order books internally
- `LiveLogger` now exhibits better blocking behavior and logging

### Fixes
- Various patches to the Betfair adapter
- Documentation builds

---

# NautilusTrader 1.116.1 Beta

## Release Notes

Announcing official Windows 64-bit support.

Several bugs have been identified and fixed.

### Breaking Changes
None

### Enhancements
- Performance test refactoring
- Removed redundant performance harness
- Added `Queue.peek()` to high-performance queue
- GitHub action refactoring, CI for Windows
- Builds for 32-bit platforms

### Fixes
- `OrderBook.create` for `BookLevel.L3` now returns correct book
- Betfair handling of trade IDs

---

# NautilusTrader 1.116.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Further fundamental changes to the core API have been made.

### Breaking Changes
- Introduce `ClientId` for data and execution client identification
- Standardized client IDs to upper case
- Renamed `OrderBookOperation` to `OrderBookDelta`
- Renamed `OrderBookOperations` to `OrderBookDeltas`
- Renamed `OrderBookOperationType` to `OrderBookDeltaType`

### Enhancements
None

### Fixes
None

---

# NautilusTrader 1.115.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Due to recent feedback and much further thought - a major renaming has been carried
out involving order identifiers. The `Order` is the only domain object in the
model which is identified with more than one ID. Due to this, more explicitness
helps to ensure correct logic. Previously the `OrderId` was
implicitly assumed to be the one assigned by the trading venue. This has been
clarified by renaming the identifier to `VenueOrderId`. Following this, it no
longer made sense to refer to it through `Order.id`, and so this was changed to
its full name `Order.venue_order_id`. This naturally resulted in `ClientOrderId`(s)
being renamed in properties and variables from `cl_ord_id` to `client_order_id`.

### Breaking Changes
- Renamed `OrderId` to `VenueOrderId`
- Renamed `Order.id` to `Order.venue_order_id`
- Renamed `Order.cl_ord_id` to `Order.client_order_id`
- Renamed `AssetClass.STOCK` to `AssetClass.EQUITY`
- Removed redundant flag `generate_position_ids` (handled by `OmsType`)

### Enhancements
- Introduce integration for Betfair.
- Added `AssetClass.METAL` and `AssetClass.ENERGY`
- Added `VenueStatusEvent`, `InstrumentStatusEvent` and `InstrumentClosePrice`
- Usage of `np.ndarray` to improve function and indicator performance

### Fixes
- LiveLogger log message when blocking.

---

# NautilusTrader 1.114.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Further standardization of naming conventions along with internal refinements
and fixes.

### Breaking Changes
- Renamed `AmendOrder` to `UpdateOrder`
- Renamed `OrderAmended` to `OrderUpdated`
- Renamed `amend` and `amended` related methods to `update` and `updated`
- Renamed `OrderCancelReject` to `OrderCancelRejected` (standardize tense)

### Enhancements
- Improve efficiency of data wrangling
- Simplify `Logger` and general system logging
- Added `stdout` and `stderr` log streams with configuration
- Added `OrderBookData` base class

### Fixes
- Backtest handling of `GenericData` and `OrderBook` related data
- Backtest `DataClient` creation logic prevented client registering

---

# NautilusTrader 1.113.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

Further standardization of naming conventions along with internal refinements
and fixes.

### Breaking Changes
- Renamed `AmendOrder` to `UpdateOrder`
- Renamed `OrderAmended` to `OrderUpdated`
- Renamed `amend` and `amended` related methods to `update` and `updated`
- Renamed `OrderCancelReject` to `OrderCancelRejected` (standardize tense)

### Enhancements
- Introduce `OrderUpdateRejected`, event separated for clarity
- Refined LiveLogger: Now runs on event loop with high-performance `Queue`
- Improved flexibility of when strategies are added to a `BacktestEngine`
- Improved checks for `VenueOrderId` equality when applying order events

### Fixes
- Removed `UNDEFINED` enum values. Do not allow invalid values to be represented
in the system (prefer throwing exceptions)

---

# NautilusTrader 1.112.0 Beta

## Release Notes

**This release includes substantial breaking changes.**

The platforms internal timestamping has been standardized to nanoseconds. This
decision was made to increase the accuracy of backtests to nanosecond precision,
improve data handling including order book and custom data for backtesting, and
to future-proof the platform to a more professional standard. The top-level user
API still takes `datetime` and `timedelta` objects for usability.

There has also been some standardization of naming conventions to align more
closely with established financial market terminology with reference to the
FIX5.0 SP2 specification, and CME MDP 3.0.

### Breaking Changes
- Moved `BarType` into `Bar` as a property
- Changed signature of `Bar` handling methods due to above
- Removed `Instrument.leverage` (incorrect place for concept)
- Changed `ExecutionClient.venue` as a `Venue` to `ExecutionClient.name` as a `str`
- Changed serialization of timestamp datatype to `int64`
- Changed serialization constant names extensively
- Renamed `OrderFilled.filled_qty` to `OrderFilled.last_qty`
- Renamed `OrderFilled.filled_price` to `OrderFilled.last_px`
- Renamed `avg_price` to `avg_px` in methods and properties
- Renamed `avg_open` to `avg_px_open` in methods and properties
- Renamed `avg_close` to `avg_px_close` in methods and properties
- Renamed `Position.relative_quantity` to `Position.relative_qty`
- Renamed `Position.peak_quantity` to `Position.peak_qty`

### Enhancements
- Standardized nanosecond timestamps
- Added time unit conversion functions as found in `nautilus_trader.core.datetime`
- Added optional `broker` property to `Venue` to assist with routing
- Enhanced state reconciliation from both `LiveExecutionEngine` and `LiveExecutionClient`
- Added internal messages to aid state reconciliation

### Fixes
- `DataCache` incorrectly caching bars

---

# NautilusTrader 1.111.0 Beta

## Release Notes

This release adds further enhancements to the platform.

### Breaking Changes
None

### Enhancements
- `RiskEngine` built out including configuration options hook and
  `LiveRiskEngine` implementation
- Added generic `Throttler`
- Added details `dict` to `instrument_id` related requests to cover IB futures
  contracts
- Added missing Fiat currencies
- Added additional Crypto currencies
- Added ISO 4217 codes
- Added currency names

### Fixes
- Queue `put` coroutines in live engines when blocking at `maxlen` was not
  creating a task on the event loop.

---

# NautilusTrader 1.110.0 Beta

## Release Notes

This release applies one more major change to the identifier API. `Security` has
been renamed to `InstrumentId` for greater clarity that the object is an identifier,
and to group the concept of an instrument with its identifier.

Data objects in the framework have been further abstracted to prepare for the
handling of custom data in backtests.

A `RiskEngine` base class has also been scaffolded.

### Breaking Changes
- `Security` renamed to `InstrumentId`
- `Instrument.security` renamed to `Instrument.id`
- `Data` becomes an abstract base class with `timestamp` and `unix_timestamp`
  properties
- `Data` and `DataType` moved to `model.data`
- `on_data` methods now take `GenericData`

### Enhancements
- Added `GenericData`
- Added`Future` instrument

### Fixes
None

---

# NautilusTrader 1.109.0 Beta

## Release Notes

The main thrust of this release is to refine and further bed down the changes
to the identifier model via `InstrumentId`, and fix some bugs.

Errors in the CCXT clients caused by the last release have been addressed.

### Breaking Changes
- `InstrumentId` now takes first class value object `Symbol`
- `InstrumentId` `asset_class` and `asset_type` no longer optional
- `SimulatedExchange.venue` changed to `SimulatedExchange.id`

### Enhancements
- Ensure `TestTimer` advances monotonically increase
- Added `AssetClass.BETTING`

### Fixes
- CCXT data and execution clients regarding `instrument_id` vs `symbol` naming
- `InstrumentId` equality and hashing
- Various docstrings

---

# NautilusTrader 1.108.0 Beta

## Release Notes

This release executes a major refactoring of `Symbol` and how securities are
generally identified within the platform. This will allow a smoother integration
with Interactive Brokers and other exchanges, brokerages and trading
counterparties.

Previously the `Symbol` identifier also included a venue which confused the concept.
The replacement `Security` identifier more clearly expresses the domain with a
symbol string, a primary `Venue`, `AssetClass` and `AssetType` properties.

### Breaking Changes
- All previous serializations
- `Security` replaces `Symbol` with expanded properties
- `AssetClass.EQUITY` changed to `AssetClass.STOCK`
- `from_serializable_string` changed to `from_serializable_str`
- `to_serializable_string` changed to `to_serializable_str`

### Enhancements
- Reports now include full instrument_id name
- Added `AssetType.WARRANT`

### Fixes
- `StopLimitOrder` serialization

---

# NautilusTrader 1.107.1 Beta - Release Notes

This is a patch release which applies various fixes and refactorings.

The behavior of the `StopLimitOrder` continued to be fixed and refined.
`SimulatedExchange` was refactored further to reduce complexity.

### Breaking Changes
None

### Enhancements
None

### Fixes
- `TRIGGERED` states in order FSM
- `StopLimitOrder` triggering behavior
- `OrderFactory.stop_limit` missing `post_only` and `hidden`
- `Order` and `StopLimitOrder` `__repr__` string (duplicate id)

---

# NautilusTrader 1.107.0 Beta

## Release Notes

The main thrust of this release is to refine some subtleties relating to order
matching and amendment behavior for improved realism. This involved a fairly substantial refactoring
of `SimulatedExchange` to manage its complexity, and support extending the order types.

The `post_only` flag for LIMIT orders now results in the expected behavior regarding
when a marketable limit order will become a liquidity `TAKER` during order placement
and amendment.

Test coverage was moderately increased.

### Breaking Changes
None

### Enhancements
- Refactored `SimulatedExchange` order matching and amendment logic
- Added `risk` subpackage to group risk components

### Fixes
- `StopLimitOrder` triggering behavior
- All flake8 warnings

---

# NautilusTrader 1.106.0 Beta

## Release Notes

The main thrust of this release is to introduce the Interactive Brokers
integration, and begin adding platform capabilities to support this effort.

### Breaking Changes
- `from_serializable_string` methods changed to `from_serializable_str`

### Enhancements
- Scaffold Interactive Brokers integration in `adapters/ib`
- Added the `Future` instrument type
- Added the `StopLimitOrder` order type
- Added the `Data` and `DataType` types to support custom data handling
- Added the `InstrumentId` identifier types initial implementation to support extending the platforms capabilities

### Fixes
- `BracketOrder` correctness
- CCXT precision parsing bug
- Some log formatting

---
