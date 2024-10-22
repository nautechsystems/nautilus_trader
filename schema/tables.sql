------------------- ENUMS -------------------

CREATE TYPE ACCOUNT_TYPE AS ENUM ('Cash', 'Margin', 'Betting');
CREATE TYPE AGGREGATION_SOURCE AS ENUM ('EXTERNAL', 'INTERNAL');
CREATE TYPE AGGRESSOR_SIDE AS ENUM ('NO_AGGRESSOR','BUYER','SELLER');
CREATE TYPE ASSET_CLASS AS ENUM ('FX', 'EQUITY', 'COMMODITY', 'DEBT', 'INDEX', 'CRYPTOCURRENCY', 'ALTERNATIVE');
CREATE TYPE INSTRUMENT_CLASS AS ENUM ('Spot', 'Swap', 'Future', 'FutureSpread', 'Forward', 'Cfg', 'Bond', 'Option', 'OptionSpread', 'Warrant', 'SportsBetting');
CREATE TYPE BAR_AGGREGATION AS ENUM ('TICK', 'TICK_IMBALANCE', 'TICK_RUNS', 'VOLUME', 'VOLUME_IMBALANCE', 'VOLUME_RUNS', 'VALUE', 'VALUE_IMBALANCE', 'VALUE_RUNS', 'MILLISECOND', 'SECOND', 'MINUTE', 'HOUR', 'DAY', 'WEEK', 'MONTH');
CREATE TYPE BOOK_ACTION AS ENUM ('Add', 'Update', 'Delete','Clear');
CREATE TYPE ORDER_STATUS AS ENUM ('Initialized', 'Denied', 'Emulated', 'Released', 'Submitted', 'Accepted', 'Rejected', 'Canceled', 'Expired', 'Triggered', 'PendingUpdate', 'PendingCancel', 'PartiallyFilled', 'Filled');
CREATE TYPE CURRENCY_TYPE AS ENUM('CRYPTO', 'FIAT', 'COMMODITY_BACKED');
CREATE TYPE TRAILING_OFFSET_TYPE AS ENUM('NO_TRAILING_OFFSET', 'PRICE', 'BASIS_POINTS', 'TICKS', 'PRICE_TIER');
CREATE TYPE PRICE_TYPE AS ENUM('BID','ASK','MID','LAST');
------------------- TABLES -------------------

CREATE TABLE IF NOT EXISTS "general" (
    id TEXT PRIMARY KEY NOT NULL,
    value bytea not null
);

CREATE TABLE IF NOT EXISTS "trader" (
    id TEXT PRIMARY KEY NOT NULL,
    instance_id UUID
);

CREATE TABLE IF NOT EXISTS "account" (
  id TEXT PRIMARY KEY NOT NULL
);

CREATE TABLE IF NOT EXISTS "client" (
    id TEXT PRIMARY KEY NOT NULL
);

CREATE TABLE IF NOT EXISTS "strategy" (
  id TEXT PRIMARY KEY NOT NULL,
  order_id_tag TEXT,
  oms_type TEXT,
  manage_contingent_orders BOOLEAN,
  manage_gtd_expiry BOOLEAN
);

CREATE TABLE IF NOT EXISTS "currency" (
    id TEXT PRIMARY KEY NOT NULL,
    precision INTEGER,
    iso4217 INTEGER,
    name TEXT,
    currency_type CURRENCY_TYPE
);

CREATE TABLE IF NOT EXISTS "instrument" (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT,
    raw_symbol TEXT NOT NULL,
    base_currency TEXT REFERENCES currency(id),
    underlying TEXT,
    quote_currency TEXT REFERENCES currency(id),
    settlement_currency TEXT REFERENCES currency(id),
    isin TEXT,
    asset_class ASSET_CLASS,
    exchange TEXT,
    multiplier TEXT,
    option_kind TEXT,
    is_inverse BOOLEAN DEFAULT FALSE,
    strike_price TEXT,
    activation_ns TEXT,
    expiration_ns TEXT,
    price_precision INTEGER NOT NULL ,
    size_precision INTEGER,
    price_increment TEXT NOT NULL,
    size_increment TEXT,
    maker_fee TEXT NULL,
    taker_fee TEXT NULL,
    margin_init TEXT NOT NULL,
    margin_maint TEXT NOT NULL,
    lot_size TEXT,
    max_quantity TEXT,
    min_quantity TEXT,
    max_notional TEXT,
    min_notional TEXT,
    max_price TEXT,
    min_price TEXT,
    ts_init TEXT NOT NULL,
    ts_event TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "order" (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    order_type TEXT,
    status TEXT,
--     trader_id TEXT REFERENCES trader(id) ON DELETE CASCADE,
--     strategy_id TEXT REFERENCES strategy(id) ON DELETE CASCADE,
--     instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    symbol TEXT,
    venue TEXT,
    venue_order_id TEXT,
    position_id TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "order_event" (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    trader_id TEXT REFERENCES trader(id) ON DELETE CASCADE,
    strategy_id TEXT NOT NULL,
    instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    client_order_id TEXT DEFAULT NULL,
    client_id TEXT REFERENCES client(id) ON DELETE CASCADE,
    trade_id TEXT,
    currency TEXT REFERENCES currency(id),
    order_type TEXT,
    order_side TEXT,
    quantity TEXT,
    time_in_force TEXT,
    liquidity_side TEXT,
    post_only BOOLEAN DEFAULT FALSE,
    reduce_only BOOLEAN DEFAULT FALSE,
    quote_quantity BOOLEAN DEFAULT FALSE,
    reconciliation BOOLEAN DEFAULT FALSE,
    price TEXT,
    last_px TEXT,
    last_qty TEXT,
    trigger_price TEXT,
    trigger_type TEXT,
    limit_offset TEXT,
    trailing_offset TEXT,
    trailing_offset_type TRAILING_OFFSET_TYPE,
    expire_time TEXT,
    display_qty TEXT,
    emulation_trigger TEXT,
    trigger_instrument_id TEXT,
    contingency_type TEXT,
    order_list_id TEXT,
    linked_order_ids TEXT[],
    parent_order_id TEXT,
    exec_algorithm_id TEXT,
    exec_algorithm_params JSONB,
    exec_spawn_id TEXT,
    venue_order_id TEXT,
    account_id TEXT,
    position_id TEXT,
    commission TEXT,
    tags TEXT[],
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "position"(
    id TEXT PRIMARY KEY NOT NULL,
    trader_id TEXT REFERENCES trader(id) ON DELETE CASCADE,
    strategy_id TEXT NOT NULL,
    instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    opening_order_id TEXT NOT NULL,
    closing_order_id TEXT,  -- REFERENCES TBD
    entry TEXT NOT NULL,
    side TEXT NOT NULL,
    signed_qty TEXT NOT NULL,
    quantity TEXT NOT NULL,
    peak_qty TEXT NOT NULL,
    -- last_qty TEXT,
    -- last_px TEXT,
    quote_currency TEXT NOT NULL,
    base_currency TEXT,
    settlement_currency TEXT NOT NULL,
    avg_px_open DOUBLE PRECISION NOT NULL,  -- Consider NUMERIC
    avg_px_close DOUBLE PRECISION,  -- Consider NUMERIC
    realized_return DOUBLE PRECISION, -- Consider NUMERIC
    realized_pnl TEXT NOT NULL,
    unrealized_pnl TEXT,
    commissions TEXT NOT NULL,
    duration_ns TEXT,
    ts_opened TEXT NOT NULL,
    ts_closed TEXT,
    ts_last TEXT NOT NULL,
    ts_init TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "account_event"(
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    account_id TEXT REFERENCES account(id) ON DELETE CASCADE,
    base_currency TEXT REFERENCES currency(id),
    balances JSONB,
    margins JSONB,
    is_reported BOOLEAN DEFAULT FALSE,
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "trade" (
    id BIGSERIAL PRIMARY KEY NOT NULL,
    instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    price TEXT NOT NULL,
    quantity TEXT NOT NULL,
    aggressor_side AGGRESSOR_SIDE,
    venue_trade_id TEXT,
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "quote" (
    id BIGSERIAL PRIMARY KEY NOT NULL,
    instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    bid_price TEXT NOT NULL,
    ask_price TEXT NOT NULL,
    bid_size TEXT NOT NULL,
    ask_size TEXT NOT NULL,
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "bar" (
    id BIGSERIAL PRIMARY KEY NOT NULL,
    instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    step INTEGER NOT NULL,
    bar_aggregation BAR_AGGREGATION NOT NULL,
    price_type PRICE_TYPE NOT NULL,
    aggregation_source AGGREGATION_SOURCE NOT NULL,
    open TEXT NOT NULL,
    high TEXT NOT NULL,
    low TEXT NOT NULL,
    close TEXT NOT NULL,
    volume TEXT NOT NULL,
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "signal" (
    id BIGSERIAL PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "custom" (
    id BIGSERIAL PRIMARY KEY NOT NULL,
    data_type TEXT NOT NULL,
    metadata JSONB NOT NULL,
    value BYTEA NOT NULL,
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);
