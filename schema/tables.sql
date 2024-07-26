------------------- ENUMS -------------------

CREATE TYPE ACCOUNT_TYPE AS ENUM ('Cash', 'Margin', 'Betting');
CREATE TYPE AGGREGATION_SOURCE AS ENUM ('External', 'Internal');
CREATE TYPE AGGRESSOR_SIDE AS ENUM ('NoAggressor', 'Buyer', 'Seller');
CREATE TYPE ASSET_CLASS AS ENUM ('FX', 'EQUITY', 'COMMODITY', 'DEBT', 'INDEX', 'CRYPTOCURRENCY', 'ALTERNATIVE');
CREATE TYPE INSTRUMENT_CLASS AS ENUM ('Spot', 'Swap', 'Future', 'FutureSpread', 'Forward', 'Cfg', 'Bond', 'Option', 'OptionSpread', 'Warrant', 'SportsBetting');
CREATE TYPE BAR_AGGREGATION AS ENUM ('Tick', 'TickImbalance', 'TickRuns', 'Volume', 'VolumeImbalance', 'VolumeRuns', 'Value', 'ValueImbalance', 'ValueRuns', 'Millisecond', 'Second', 'Minute', 'Hour', 'Day', 'Week', 'Month');
CREATE TYPE BOOK_ACTION AS ENUM ('Add', 'Update', 'Delete','Clear');
CREATE TYPE ORDER_STATUS AS ENUM ('Initialized', 'Denied', 'Emulated', 'Released', 'Submitted', 'Accepted', 'Rejected', 'Canceled', 'Expired', 'Triggered', 'PendingUpdate', 'PendingCancel', 'PartiallyFilled', 'Filled');
CREATE TYPE CURRENCY_TYPE AS ENUM('CRYPTO', 'FIAT', 'COMMODITY_BACKED');

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
    instrument_id TEXT NOT NULL,
    order_id TEXT DEFAULT NULL,
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
    trailing_offset_type TEXT,
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
