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
    asset_class ASSET_CLASS,
    underlying TEXT,
    base_currency TEXT REFERENCES currency(id),
    quote_currency TEXT REFERENCES currency(id),
    settlement_currency TEXT REFERENCES currency(id),
    isin TEXT,
    exchange TEXT,
    option_kind TEXT,
    strike_price TEXT,
    activation_ns TEXT,
    expiration_ns TEXT,
    price_precision INTEGER NOT NULL ,
    size_precision INTEGER,
    price_increment TEXT NOT NULL,
    size_increment TEXT,
    is_inverse BOOLEAN DEFAULT FALSE,
    multiplier TEXT,
    lot_size TEXT,
    max_quantity TEXT,
    min_quantity TEXT,
    max_notional TEXT,
    min_notional TEXT,
    max_price TEXT,
    min_price TEXT,
    margin_init TEXT NOT NULL,
    margin_maint TEXT NOT NULL,
    maker_fee TEXT NULL,
    taker_fee TEXT NULL,
    ts_event TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "order" (
    id TEXT PRIMARY KEY NOT NULL,
    trader_id TEXT REFERENCES trader(id) ON DELETE CASCADE,
    strategy_id TEXT NOT NULL,
    instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    client_order_id TEXT NOT NULL,
    venue_order_id TEXT,
    position_id TEXT,
    account_id TEXT,  -- REFERENCES account(id) ON DELETE CASCADE,
    last_trade_id TEXT,
    order_type TEXT NOT NULL,
    order_side TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price TEXT,
    trigger_price TEXT,
    trigger_type TEXT,
    limit_offset TEXT,
    trailing_offset TEXT,
    trailing_offset_type TEXT,
    time_in_force TEXT NOT NULL,
    expire_time TEXT,
    filled_qty TEXT DEFAULT '0',
    liquidity_side TEXT,
    avg_px DOUBLE PRECISION,
    slippage DOUBLE PRECISION,
    commissions TEXT[],
    status TEXT NOT NULL,
    is_post_only BOOLEAN,
    is_reduce_only BOOLEAN,
    is_quote_quantity BOOLEAN,
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
    tags TEXT[],
    init_id TEXT NOT NULL,
    ts_init TEXT NOT NULL,
    ts_last TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS "order_event" (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    trader_id TEXT REFERENCES trader(id) ON DELETE CASCADE,
    strategy_id TEXT NOT NULL,
    instrument_id TEXT REFERENCES instrument(id) ON DELETE CASCADE,
    client_order_id TEXT NOT NULL,
    client_id TEXT REFERENCES client(id) ON DELETE CASCADE,
    reason TEXT,
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
    signed_qty DOUBLE PRECISION NOT NULL,
    quantity TEXT NOT NULL,
    peak_qty TEXT NOT NULL,
    quote_currency TEXT NOT NULL,
    base_currency TEXT,
    settlement_currency TEXT NOT NULL,
    avg_px_open DOUBLE PRECISION NOT NULL,
    avg_px_close DOUBLE PRECISION,
    realized_return DOUBLE PRECISION,
    realized_pnl TEXT,
    unrealized_pnl TEXT,
    commissions TEXT[],
    duration_ns TEXT,
    ts_opened TEXT NOT NULL,
    ts_closed TEXT,
    ts_init TEXT NOT NULL,
    ts_last TEXT NOT NULL,
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

------------------- BLOCKCHAIN -------------------

CREATE TABLE IF NOT EXISTS "chain" (
    chain_id INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "block" (
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    number BIGINT NOT NULL,
    hash TEXT,
    parent_hash TEXT,
    miner TEXT,
    gas_limit BIGINT,
    gas_used BIGINT,
    timestamp TEXT,
    base_fee_per_gas TEXT,
    blob_gas_used TEXT,
    excess_blob_gas TEXT,
    l1_gas_price TEXT,
    l1_gas_used BIGINT,
    l1_fee_scalar BIGINT,
    PRIMARY KEY (chain_id, number)
) PARTITION BY LIST (chain_id);
CREATE TABLE IF NOT EXISTS "block_default" PARTITION OF "block" DEFAULT;

CREATE TABLE IF NOT EXISTS "token"(
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    address TEXT NOT NULL,
    symbol TEXT,
    name TEXT,
    decimals INTEGER,
    error TEXT,
    PRIMARY KEY (chain_id, address)
) PARTITION BY LIST (chain_id);
CREATE TABLE IF NOT EXISTS "token_default" PARTITION OF "token" DEFAULT;

CREATE TABLE IF NOT EXISTS "dex" (
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    factory_address TEXT UNIQUE,
    creation_block BIGINT NOT NULL,
    last_full_sync_pools_block_number BIGINT,
    PRIMARY KEY (chain_id, name)
);

CREATE TABLE IF NOT EXISTS "pool" (
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    dex_name TEXT NOT NULL,
    address TEXT NOT NULL,
    creation_block BIGINT NOT NULL,
    token0_chain INTEGER NOT NULL,
    token0_address TEXT NOT NULL,
    token1_chain INTEGER NOT NULL,
    token1_address TEXT NOT NULL,
    fee INTEGER,
    tick_spacing INTEGER,
    initial_tick INTEGER,
    initial_sqrt_price_x96 TEXT,
    last_full_sync_block_number BIGINT,
    PRIMARY KEY (chain_id, address),
    FOREIGN KEY (token0_chain, token0_address) REFERENCES token(chain_id, address),
    FOREIGN KEY (token1_chain, token1_address) REFERENCES token(chain_id, address),
    FOREIGN KEY (chain_id, dex_name) REFERENCES dex(chain_id, name)
);

CREATE TABLE IF NOT EXISTS "pool_swap_event" (
    id BIGSERIAL PRIMARY KEY,
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    pool_address TEXT NOT NULL,
    block BIGINT NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index INTEGER NOT NULL,
    log_index INTEGER NOT NULL,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    side TEXT NOT NULL,
    size TEXT NOT NULL,
    price TEXT NOT NULL,
    sqrt_price_x96 U160 NOT NULL,
    liquidity U128 NOT NULL,
    tick INTEGER NOT NULL,
    amount0 I256 NOT NULL,
    amount1 I256 NOT NULL,
    FOREIGN KEY (chain_id, pool_address) REFERENCES pool(chain_id, address),
--     FOREIGN KEY (chain_id, block) REFERENCES block(chain_id, number), // TODO temporarily disabled not to be blocked by full block sync
    UNIQUE(chain_id, transaction_hash, log_index)
);
CREATE INDEX IF NOT EXISTS idx_pool_swap_event_lookup
    ON pool_swap_event(chain_id, pool_address, block, transaction_index, log_index);

CREATE TABLE IF NOT EXISTS "pool_liquidity_event" (
    id BIGSERIAL PRIMARY KEY,
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    pool_address TEXT NOT NULL,
    block BIGINT NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index INTEGER NOT NULL,
    log_index INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    sender TEXT,
    owner TEXT NOT NULL,
    position_liquidity U128 NOT NULL,
    amount0 U160 NOT NULL,
    amount1 U160 NOT NULL,
    tick_lower INTEGER NOT NULL,
    tick_upper INTEGER NOT NULL,
    FOREIGN KEY (chain_id, pool_address) REFERENCES pool(chain_id, address),
--     FOREIGN KEY (chain_id, block) REFERENCES block(chain_id, number),  // TODO temporarily disabled not to be blocked by full block sync
    UNIQUE(chain_id, transaction_hash, log_index)
);
CREATE INDEX IF NOT EXISTS idx_pool_liquidity_event_lookup
    ON pool_liquidity_event(chain_id, pool_address, block, transaction_index, log_index);

CREATE TABLE IF NOT EXISTS "pool_collect_event" (
    id BIGSERIAL PRIMARY KEY,
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    pool_address TEXT NOT NULL,
    block BIGINT NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index INTEGER NOT NULL,
    log_index INTEGER NOT NULL,
    owner TEXT NOT NULL,
    amount0 U256 NOT NULL,
    amount1 U256 NOT NULL,
    tick_lower INTEGER NOT NULL,
    tick_upper INTEGER NOT NULL,
    FOREIGN KEY (chain_id, pool_address) REFERENCES pool(chain_id, address),
--     FOREIGN KEY (chain_id, block) REFERENCES block(chain_id, number),  // TODO temporarily disabled not to be blocked by full block sync
    UNIQUE(chain_id, transaction_hash, log_index)
);
CREATE INDEX IF NOT EXISTS idx_pool_collect_event_lookup
    ON pool_collect_event(chain_id, pool_address, block, transaction_index, log_index);

CREATE TABLE IF NOT EXISTS "pool_flash_event" (
    id BIGSERIAL PRIMARY KEY,
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    pool_address TEXT NOT NULL,
    block BIGINT NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index INTEGER NOT NULL,
    log_index INTEGER NOT NULL,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    amount0 U256 NOT NULL,
    amount1 U256 NOT NULL,
    paid0 U256 NOT NULL,
    paid1 U256 NOT NULL,
    FOREIGN KEY (chain_id, pool_address) REFERENCES pool(chain_id, address),
--     FOREIGN KEY (chain_id, block) REFERENCES block(chain_id, number),  // TODO temporarily disabled not to be blocked by full block sync
    UNIQUE(chain_id, transaction_hash, log_index)
);
CREATE INDEX IF NOT EXISTS idx_pool_flash_event_lookup
    ON pool_flash_event(chain_id, pool_address, block, transaction_index, log_index);

CREATE TABLE IF NOT EXISTS "pool_snapshot" (
    chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
    pool_address TEXT NOT NULL,
    block BIGINT NOT NULL,
    transaction_index INTEGER NOT NULL,
    log_index INTEGER NOT NULL,
    transaction_hash TEXT NOT NULL,
    current_tick INTEGER NOT NULL,
    price_sqrt_ratio_x96 U160 NOT NULL,
    liquidity U128 NOT NULL,
    protocol_fees_token0 U256 NOT NULL,
    protocol_fees_token1 U256 NOT NULL,
    fee_protocol SMALLINT NOT NULL,
    fee_growth_global_0 U256 NOT NULL,
    fee_growth_global_1 U256 NOT NULL,
    total_amount0_deposited U256 NOT NULL,
    total_amount1_deposited U256 NOT NULL,
    total_amount0_collected U256 NOT NULL,
    total_amount1_collected U256 NOT NULL,
    total_swaps INTEGER NOT NULL DEFAULT 0,
    total_mints INTEGER NOT NULL DEFAULT 0,
    total_burns INTEGER NOT NULL DEFAULT 0,
    total_flashes INTEGER NOT NULL DEFAULT 0,
    total_fee_collects INTEGER NOT NULL,
    liquidity_utilization_rate  DOUBLE PRECISION DEFAULT 0,
    is_valid BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (chain_id, pool_address, block, transaction_index, log_index),
    FOREIGN KEY (chain_id, pool_address) REFERENCES pool(chain_id, address)
);

CREATE TABLE IF NOT EXISTS "pool_position" (
    chain_id INTEGER NOT NULL,
    pool_address TEXT NOT NULL,
    snapshot_block BIGINT NOT NULL,
    snapshot_transaction_index INTEGER NOT NULL,
    snapshot_log_index INTEGER NOT NULL,
    owner TEXT NOT NULL,
    tick_lower INTEGER NOT NULL,
    tick_upper INTEGER NOT NULL,
    liquidity U128 NOT NULL,
    fee_growth_inside_0_last U256 NOT NULL,
    fee_growth_inside_1_last U256 NOT NULL,
    tokens_owed_0 U128 NOT NULL,
    tokens_owed_1 U128 NOT NULL,
    total_amount0_deposited U256,
    total_amount1_deposited U256,
    total_amount0_collected U128,
    total_amount1_collected U128,
    is_consistent BOOLEAN DEFAULT FALSE,
    PRIMARY KEY (chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index, owner, tick_lower, tick_upper),
    FOREIGN KEY (chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index)
        REFERENCES pool_snapshot(chain_id, pool_address, block, transaction_index, log_index) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS "pool_tick" (
    chain_id INTEGER NOT NULL,
    pool_address TEXT NOT NULL,
    snapshot_block BIGINT NOT NULL,
    snapshot_transaction_index INTEGER NOT NULL,
    snapshot_log_index INTEGER NOT NULL,
    tick_value INTEGER NOT NULL,
    liquidity_gross U128 NOT NULL,
    liquidity_net I128 NOT NULL,
    fee_growth_outside_0 U256 NOT NULL,
    fee_growth_outside_1 U256 NOT NULL,
    initialized BOOLEAN NOT NULL,
    last_updated_block BIGINT NOT NULL,
    PRIMARY KEY (chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index, tick_value),
    FOREIGN KEY (chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index)
        REFERENCES pool_snapshot(chain_id, pool_address, block, transaction_index, log_index) ON DELETE CASCADE
);
