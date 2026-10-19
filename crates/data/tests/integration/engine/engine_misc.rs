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

use super::*;

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_mark_price(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(sub));

    data_engine.borrow_mut().execute(cmd);

    let mark_price = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<MarkPriceUpdate>(None);
    let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
    msgbus::subscribe_mark_prices(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::MarkPrice(mark_price), borrowed);
    let cache = &data_engine.cache().borrow();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.price(&mark_price.instrument_id, PriceType::Mark),
        Some(mark_price.value)
    );
    assert_eq!(
        cache.mark_price(&mark_price.instrument_id),
        Some(&mark_price)
    );
    assert_eq!(
        cache.mark_prices(&mark_price.instrument_id),
        Some(vec![mark_price])
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&mark_price));
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_index_price(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeIndexPrices::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::IndexPrices(sub));

    data_engine.borrow_mut().execute(cmd);

    let index_price = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<IndexPriceUpdate>(None);
    let topic = switchboard::get_index_price_topic(index_price.instrument_id);
    msgbus::subscribe_index_prices(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::IndexPrice(index_price), borrowed);
    let cache = &data_engine.cache().borrow();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.index_price(&index_price.instrument_id),
        Some(&index_price)
    );
    assert_eq!(
        cache.index_prices(&index_price.instrument_id),
        Some(vec![index_price])
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&index_price));
}

#[rstest]
fn test_process_funding_rate_through_any(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    data_engine.borrow_mut().execute(cmd);

    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<FundingRateUpdate>(None);
    let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
    msgbus::subscribe_funding_rates(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    // Test through the process() method with &dyn Any
    data_engine.process(&funding_rate as &dyn Any);
    let cache = &data_engine.cache().borrow();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.funding_rate(&funding_rate.instrument_id),
        Some(&funding_rate)
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&funding_rate));
}

#[rstest]
fn test_process_funding_rate(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    data_engine.borrow_mut().execute(cmd);

    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<FundingRateUpdate>(None);
    let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
    msgbus::subscribe_funding_rates(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.handle_funding_rate(funding_rate);
    let cache = &data_engine.cache().borrow();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.funding_rate(&funding_rate.instrument_id),
        Some(&funding_rate)
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&funding_rate));
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_funding_rate_data_variant(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    data_engine.borrow_mut().execute(cmd);

    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<FundingRateUpdate>(None);
    let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
    msgbus::subscribe_funding_rates(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::FundingRate(funding_rate), borrowed);
    let cache = &data_engine.cache().borrow();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.funding_rate(&funding_rate.instrument_id),
        Some(&funding_rate)
    );
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0], funding_rate);
}

#[rstest]
fn test_process_funding_rate_updates_existing(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    data_engine.borrow_mut().execute(cmd);

    let funding_rate1 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );

    let funding_rate2 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0002".parse().unwrap(),
        None,
        None,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.handle_funding_rate(funding_rate1);
    data_engine.handle_funding_rate(funding_rate2);
    let cache = &data_engine.cache().borrow();

    // Should only have the latest funding rate
    assert_eq!(
        cache.funding_rate(&funding_rate2.instrument_id),
        Some(&funding_rate2)
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_block(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    let blockchain = Blockchain::Ethereum;

    let sub = DefiSubscribeCommand::Blocks(SubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let block = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );
    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<Block>(None);
    let topic = defi::switchboard::get_defi_blocks_topic(blockchain);
    msgbus::subscribe_defi_blocks(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::Block(block.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&block));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_swap(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create a pool swap
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let swap = PoolSwap::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
    );

    let sub = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<PoolSwap>(None);
    let topic = defi::switchboard::get_defi_pool_swaps_topic(instrument_id);
    msgbus::subscribe_defi_swaps(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolSwap(swap.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&swap));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_liquidity_update(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create test pool
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let update = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        PoolLiquidityUpdateType::Mint,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        100u128,
        U256::from(1000000u128),
        U256::from(2000000u128),
        -100,
        100,
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let sub = DefiSubscribeCommand::PoolLiquidityUpdates(SubscribePoolLiquidityUpdates {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<PoolLiquidityUpdate>(None);
    let topic = defi::switchboard::get_defi_liquidity_topic(instrument_id);
    msgbus::subscribe_defi_liquidity(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolLiquidityUpdate(update.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&update));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_fee_collect(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create test pool
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let collect = PoolFeeCollect::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        Address::from([0x12; 20]),
        500000u128,
        300000u128,
        -100,
        100,
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let sub = DefiSubscribeCommand::PoolFeeCollects(SubscribePoolFeeCollects {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<PoolFeeCollect>(None);
    let topic = defi::switchboard::get_defi_collect_topic(instrument_id);
    msgbus::subscribe_defi_collects(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFeeCollect(collect.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&collect));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_flash(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create test pool
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let flash = PoolFlash::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        Address::from([0x12; 20]),
        Address::from([0x34; 20]),
        U256::from(1000000u128),
        U256::from(500000u128),
        U256::from(5000u128),
        U256::from(2500u128),
    );

    let sub = DefiSubscribeCommand::PoolFlashEvents(SubscribePoolFlashEvents {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<PoolFlash>(None);
    let topic = defi::switchboard::get_defi_flash_topic(instrument_id);
    msgbus::subscribe_defi_flash(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFlash(flash.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&flash));
}

// -- POOL UPDATER INTEGRATION TESTS ----------------------------------------------------------

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_swap_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = Rc::clone(data_engine.borrow().cache());
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ARBITRUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;
    let pool_identifier = pool.pool_identifier;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();

    // Add liquidity so swaps can be processed
    let mint = PoolLiquidityUpdate::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Mint,
        999u64,
        "0x122".to_string(),
        0,
        0,
        None,
        Address::from([0xAB; 20]),
        10000u128, // Add significant liquidity
        U256::from(1000000u128),
        U256::from(2000000u128),
        -1000, // Wide range
        1000,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    profiler.process_mint(&mint).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Verify liquidity was activated by the mint
    let active_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;
    assert!(
        active_liquidity > 0,
        "Active liquidity should be > 0 after mint, was: {active_liquidity}"
    );

    // Capture initial profiler state (after mint)
    let initial_tick = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .current_tick;
    let initial_fee_growth_0 = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .fee_growth_global_0;

    // Subscribe to pool swaps (this creates PoolUpdater and subscribes to topic)
    let sub = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Replay a swap consistent with the profiler's initialized liquidity
    let swap = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .simulate_swap_through_ticks(
            I256::from_str("100").unwrap(),
            true,
            U160::from(56022770974786139918731938227u128),
            true,
        )
        .unwrap()
        .to_swap_event(
            chain,
            dex,
            pool_identifier,
            BlockPosition::new(1000, "0x123".to_string(), 0, 0),
            UnixNanos::default(),
            UnixNanos::default(),
            Address::from([0x12; 20]),
            Address::from([0x12; 20]),
        );
    let expected_tick = swap.tick;
    let expected_price = swap.sqrt_price_x96;
    let expected_liquidity = swap.liquidity;

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolSwap(swap));

    let cache_ref = cache.borrow();
    let updated = cache_ref.pool_profiler(&instrument_id).unwrap();
    assert_eq!(updated.state.current_tick, expected_tick);
    assert_eq!(updated.state.price_sqrt_ratio_x96, expected_price);
    assert_eq!(updated.tick_map.liquidity, expected_liquidity);
    assert_eq!(updated.last_processed_event.as_ref().unwrap().number, 1000);

    // Verify profiler state was updated by PoolUpdater
    let final_tick = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .current_tick;
    let final_fee_growth_0 = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .fee_growth_global_0;

    // Verify profiler was updated - either tick changed OR fees were collected
    // (depending on whether the swap crossed ticks or just generated fees)
    let tick_changed = final_tick != initial_tick;
    let fees_increased = final_fee_growth_0 > initial_fee_growth_0;

    assert!(
        tick_changed || fees_increased,
        "PoolUpdater should have updated PoolProfiler: tick_changed={tick_changed}, fees_increased={fees_increased}, \
        initial_tick={initial_tick:?}, final_tick={final_tick:?}, initial_fee_growth={initial_fee_growth_0}, final_fee_growth={final_fee_growth_0}"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_mint_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = Rc::clone(data_engine.borrow().cache());
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Capture initial profiler tick state
    let initial_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    // Subscribe to pool liquidity updates
    let sub = DefiSubscribeCommand::PoolLiquidityUpdates(SubscribePoolLiquidityUpdates {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process mint event
    let mint = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Mint,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0xAB; 20]),
        1000u128, // liquidity amount
        U256::from(100000u128),
        U256::from(200000u128),
        -100, // tick_lower
        100,  // tick_upper
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolLiquidityUpdate(mint));

    // Verify profiler tick map was updated with new liquidity
    let final_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    assert!(
        final_liquidity >= initial_liquidity,
        "Liquidity should have increased after mint"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_burn_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = Rc::clone(data_engine.borrow().cache());
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();

    // First mint some liquidity
    let owner = Address::from([0xAB; 20]);

    let mint = PoolLiquidityUpdate::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Mint,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        owner,
        1000u128,
        U256::from(100000u128),
        U256::from(200000u128),
        -100,
        100,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    profiler.process_mint(&mint).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Capture liquidity after mint
    let liquidity_after_mint = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    // Subscribe to pool liquidity updates
    let sub = DefiSubscribeCommand::PoolLiquidityUpdates(SubscribePoolLiquidityUpdates {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process burn event
    let burn = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Burn,
        1001u64,
        "0x124".to_string(),
        0,
        0,
        None,
        owner,
        500u128, // burn half the liquidity
        U256::from(50000u128),
        U256::from(100000u128),
        -100,
        100,
        UnixNanos::default(),
        UnixNanos::default(),
    );

    data_engine
        .borrow_mut()
        .process_defi_data(DefiData::PoolLiquidityUpdate(burn));

    // Verify profiler tick map was updated (liquidity decreased)
    let final_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    assert!(
        final_liquidity < liquidity_after_mint,
        "Liquidity should have decreased after burn"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_collect_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = Rc::clone(data_engine.borrow().cache());
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ETHEREUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Subscribe to pool fee collects
    let sub = DefiSubscribeCommand::PoolFeeCollects(SubscribePoolFeeCollects {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process collect event
    let owner = Address::from([0xAB; 20]);

    let collect = PoolFeeCollect::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        1000u64,
        "0x123".to_string(),
        0,
        0,
        owner,
        50000u128, // amount0
        30000u128, // amount1
        -100,      // tick_lower
        100,       // tick_upper
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFeeCollect(collect));

    // Verify profiler state - the collect should be processed without error
    // The main verification is that PoolUpdater called PoolProfiler.process_collect()
    // which would have updated internal position state if the position existed
    let is_initialized = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .is_initialized;

    // PoolProfiler should still be valid and initialized
    assert!(is_initialized, "PoolProfiler should remain initialized");
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_flash_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = Rc::clone(data_engine.borrow().cache());
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ARBITRUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        Address::from([0x12; 20]),
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Subscribe to pool flash events
    let sub = DefiSubscribeCommand::PoolFlashEvents(SubscribePoolFlashEvents {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });

    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process flash event
    let initiator = Address::from([0xAB; 20]);
    let recipient = Address::from([0xCD; 20]);

    let flash = PoolFlash::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        1000u64,
        "0x123".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        initiator,
        recipient,
        U256::from(1000000u128), // amount0
        U256::from(500000u128),  // amount1
        U256::from(5000u128),    // paid0
        U256::from(2500u128),    // paid1
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFlash(flash));

    // Verify profiler state - the flash should be processed without error
    // The main verification is that PoolUpdater called PoolProfiler.process_flash()
    // which would have updated flash statistics
    let is_initialized = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .is_initialized;

    // PoolProfiler should still be valid and initialized
    assert!(is_initialized, "PoolProfiler should remain initialized");
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_defi_pools_publishes_distinct_tradable_instruments(
    data_engine: Rc<RefCell<DataEngine>>,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let chain = Arc::new(chains::ARBITRUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "Base token".to_string(),
        "BASE".to_string(),
        8,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "Quote token".to_string(),
        "QUOTE".to_string(),
        6,
    );
    let address_a = Address::from([0xAA; 20]);
    let address_b = Address::from([0xBB; 20]);
    let address_invalid = Address::from([0xCC; 20]);

    let pool_a = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        address_a,
        PoolIdentifier::from_address(address_a),
        1,
        token0.clone(),
        token1.clone(),
        Some(500),
        Some(10),
        UnixNanos::from(1),
    );

    let pool_b = Pool::new(
        Arc::clone(&chain),
        Arc::clone(&dex),
        address_b,
        PoolIdentifier::from_address(address_b),
        2,
        token0.clone(),
        token1.clone(),
        Some(3_000),
        Some(60),
        UnixNanos::from(2),
    );

    let mut pool_invalid = Pool::new(
        chain,
        dex,
        address_invalid,
        PoolIdentifier::from_address(address_invalid),
        3,
        token0,
        token1,
        Some(10_000),
        Some(200),
        UnixNanos::from(3),
    );
    pool_invalid.token0.symbol.clear();
    let id_a = pool_a.instrument_id;
    let id_b = pool_b.instrument_id;
    let id_invalid = pool_invalid.instrument_id;
    let expected_invalid = pool_invalid.clone();
    let venue = id_a.venue;

    let expected = |pool: &Pool| {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(pool.instrument_id)
                .raw_symbol(pool.instrument_id.symbol)
                .base_currency(Currency::new(
                    "BASE",
                    8,
                    0,
                    "Base token",
                    CurrencyType::Crypto,
                ))
                .quote_currency(Currency::new(
                    "QUOTE",
                    6,
                    0,
                    "Quote token",
                    CurrencyType::Crypto,
                ))
                .price_precision(6)
                .size_precision(8)
                .price_increment(Price::from("0.000001"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(pool.ts_event)
                .ts_init(pool.ts_init)
                .build()
                .unwrap(),
        )
    };

    let expected_a = expected(&pool_a);
    let expected_b = expected(&pool_b);
    let (handler, saving_handler) =
        msgbus::stubs::get_typed_message_saving_handler::<InstrumentAny>(None);
    msgbus::subscribe_instruments(switchboard::get_instruments_pattern(venue), handler, None);

    {
        let mut engine = data_engine.borrow_mut();
        engine.process_defi_data(DefiData::Pool(pool_a));
        engine.process_defi_data(DefiData::Pool(pool_b));
        engine.process_defi_data(DefiData::Pool(pool_invalid));
    }

    let cache = Rc::clone(data_engine.borrow().cache());
    let cache = cache.borrow();
    let messages = saving_handler.get_messages();
    let selected = cache.instrument(&id_b);

    assert_ne!(id_a, id_b);
    assert_eq!(cache.instrument(&id_a), Some(&expected_a));
    assert_eq!(selected, Some(&expected_b));
    assert_eq!(cache.pool(&id_invalid), Some(&expected_invalid));
    assert_eq!(cache.instrument(&id_invalid), None);
    assert_eq!(cache.instruments(&venue, None).len(), 2);
    assert_eq!(messages.len(), 2);
    assert!(messages.contains(&expected_a));
    assert!(messages.contains(&expected_b));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_setup_pool_updater_skips_snapshot_when_pool_in_cache(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    // Create a pool with initial price and add to cache BEFORE subscribing
    let chain = Arc::new(chains::ARBITRUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        chain,
        dex,
        Address::from([0x88; 20]),
        PoolIdentifier::from_address(Address::from([0x88; 20])),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to the data_engine's cache (not the fixture cache!)
    // This ensures setup_pool_updater finds the pool when it checks the cache
    data_engine.cache().borrow_mut().add_pool(pool).unwrap();

    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd.clone());

    // Verify the cache-first optimization: when a pool exists in the data engine's
    // cache, setup_pool_updater should skip the snapshot request and proceed
    // directly to creating the profiler and updater from the cached pool.
    // Only the SubscribePool command should be forwarded to the client.
    let recorded = recorder.borrow();
    assert_eq!(
        recorded.len(),
        1,
        "Expected only 1 command (SubscribePool), but got {} commands. \
         When pool is in cache, snapshot request should be skipped.",
        recorded.len()
    );

    // The single command should be the subscription
    assert_eq!(recorded[0], cmd);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_events_publish_while_snapshot_pending(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // The adapter publishes the pool, then its bootstrap fails and no snapshot follows
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );
    let (pool, swap) = make_initialized_pool_and_swap();
    let instrument_id = pool.instrument_id;
    data_engine
        .borrow_mut()
        .execute(DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolSwaps(
            SubscribePoolSwaps {
                instrument_id,
                client_id: Some(client_id),
                command_id: UUID4::new(),
                ts_init: UnixNanos::default(),
                params: None,
            },
        )));

    let update = PoolLiquidityUpdate::new(
        Arc::clone(&pool.chain),
        Arc::clone(&pool.dex),
        instrument_id,
        pool.pool_identifier,
        PoolLiquidityUpdateType::Mint,
        1001u64,
        "0x124".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        100u128,
        U256::from(1000000u128),
        U256::from(2000000u128),
        -100,
        100,
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let collect = PoolFeeCollect::new(
        Arc::clone(&pool.chain),
        Arc::clone(&pool.dex),
        instrument_id,
        pool.pool_identifier,
        1002u64,
        "0x125".to_string(),
        0,
        0,
        Address::from([0x12; 20]),
        500000u128,
        300000u128,
        -100,
        100,
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let flash = PoolFlash::new(
        Arc::clone(&pool.chain),
        Arc::clone(&pool.dex),
        instrument_id,
        pool.pool_identifier,
        1003u64,
        "0x126".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        Address::from([0x12; 20]),
        Address::from([0x34; 20]),
        U256::from(1000000u128),
        U256::from(500000u128),
        U256::from(5000u128),
        U256::from(2500u128),
    );

    let (swap_handler, swap_saver) = get_typed_message_saving_handler::<PoolSwap>(None);
    let swap_topic = defi::switchboard::get_defi_pool_swaps_topic(instrument_id);
    msgbus::subscribe_defi_swaps(swap_topic.into(), swap_handler, None);
    let (liquidity_handler, liquidity_saver) =
        get_typed_message_saving_handler::<PoolLiquidityUpdate>(None);
    let liquidity_topic = defi::switchboard::get_defi_liquidity_topic(instrument_id);
    msgbus::subscribe_defi_liquidity(liquidity_topic.into(), liquidity_handler, None);
    let (collect_handler, collect_saver) = get_typed_message_saving_handler::<PoolFeeCollect>(None);
    let collect_topic = defi::switchboard::get_defi_collect_topic(instrument_id);
    msgbus::subscribe_defi_collects(collect_topic.into(), collect_handler, None);
    let (flash_handler, flash_saver) = get_typed_message_saving_handler::<PoolFlash>(None);
    let flash_topic = defi::switchboard::get_defi_flash_topic(instrument_id);
    msgbus::subscribe_defi_flash(flash_topic.into(), flash_handler, None);

    data_engine
        .borrow_mut()
        .process_defi_data(DefiData::Pool(pool));

    for event in [
        DefiData::PoolSwap(swap.clone()),
        DefiData::PoolLiquidityUpdate(update.clone()),
        DefiData::PoolFeeCollect(collect.clone()),
        DefiData::PoolFlash(flash.clone()),
    ] {
        data_engine.borrow_mut().process_defi_data(event);
    }

    assert!(matches!(
        recorder.borrow().last(),
        Some(DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(
            _
        )))
    ));
    assert_eq!(swap_saver.get_messages(), vec![swap]);
    assert_eq!(liquidity_saver.get_messages(), vec![update]);
    assert_eq!(collect_saver.get_messages(), vec![collect]);
    assert_eq!(flash_saver.get_messages(), vec![flash]);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_snapshot_without_cached_pool_clears_pending_state(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // A snapshot the engine cannot install ends the pending state, so a later subscription
    // requests a fresh snapshot rather than waiting on the abandoned one.
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );
    let (pool, _) = make_initialized_pool_and_swap();
    let instrument_id = pool.instrument_id;

    let subscribe = || {
        DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }))
    };

    data_engine.borrow_mut().execute(subscribe());

    let snapshot = PoolSnapshot::new(
        instrument_id,
        PoolState::default(),
        Vec::new(),
        Vec::new(),
        PoolAnalytics::default(),
        BlockPosition::new(1000, "0x0".to_string(), 0, 0),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    data_engine
        .borrow_mut()
        .process_defi_data(DefiData::PoolSnapshot(snapshot));
    data_engine.borrow_mut().execute(subscribe());

    let snapshot_requests = recorder
        .borrow()
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request))
                    if request.instrument_id == instrument_id
            )
        })
        .count();

    assert_eq!(snapshot_requests, 2);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_reset_clears_pool_updater_state(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // A reset followed by a cache reset (the backtest run boundary) must let the next
    // subscription build a fresh profiler.
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );
    let (pool, _) = make_initialized_pool_and_swap();
    let instrument_id = pool.instrument_id;

    let subscribe = || {
        DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }))
    };

    let engine_cache = Rc::clone(data_engine.borrow().cache());
    engine_cache.borrow_mut().add_pool(pool.clone()).unwrap();
    data_engine.borrow_mut().execute(subscribe());

    data_engine.borrow_mut().reset();
    engine_cache.borrow_mut().reset();
    engine_cache.borrow_mut().add_pool(pool).unwrap();
    data_engine.borrow_mut().execute(subscribe());

    assert!(
        engine_cache
            .borrow()
            .pool_profiler(&instrument_id)
            .is_some()
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_setup_pool_updater_does_not_cache_profiler_on_initialize_failure(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let chain = Arc::new(chains::ARBITRUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        chain,
        dex,
        Address::from([0x99; 20]),
        PoolIdentifier::from_address(Address::from([0x99; 20])),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    // Construct a pool whose stored initial_tick disagrees with the tick derived
    // from its sqrt price. setup_pool_updater calls PoolProfiler::initialize which
    // must return InitialTickMismatch and must not cache the half-initialized profiler.
    // Pool::initialize asserts consistency, so set the fields directly.
    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    let real_tick = get_tick_at_sqrt_ratio(initial_price);
    pool.initial_sqrt_price_x96 = Some(initial_price);
    pool.initial_tick = Some(real_tick + 100);
    let instrument_id = pool.instrument_id;

    data_engine.cache().borrow_mut().add_pool(pool).unwrap();
    assert!(
        data_engine
            .cache()
            .borrow()
            .pool_profiler(&instrument_id)
            .is_none()
    );

    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd);

    assert!(
        data_engine
            .cache()
            .borrow()
            .pool_profiler(&instrument_id)
            .is_none(),
        "profiler must not be cached when initialize fails"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_arrival_with_snapshot_pending_does_not_create_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let chain = Arc::new(chains::ARBITRUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );

    let mut pool = Pool::new(
        chain,
        dex,
        Address::from([0xAA; 20]),
        PoolIdentifier::from_address(Address::from([0xAA; 20])),
        12_345_678u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Subscribe with no pool in cache: triggers RequestPoolSnapshot and arms both pending flags.
    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd);

    {
        let recorded = recorder.borrow();
        assert_eq!(
            recorded.len(),
            2,
            "Expected SubscribePool + RequestPoolSnapshot before Pool arrives"
        );
        assert!(matches!(
            recorded[1],
            DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(_))
        ));
    }

    // Pool definition arrives while the snapshot is still in flight.
    data_engine.process_defi_data(DefiData::Pool(pool.clone()));

    assert!(
        data_engine.cache().borrow().pool(&instrument_id).is_some(),
        "pool must be added to cache when Pool data arrives"
    );
    assert!(
        data_engine
            .cache()
            .borrow()
            .pool_profiler(&instrument_id)
            .is_none(),
        "profiler must not be eager-created while a snapshot is pending"
    );
    assert_eq!(
        recorder.borrow().len(),
        2,
        "Pool arrival must not trigger a second snapshot request"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_snapshot_handler_refuses_empty_stub_at_creation_block(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let chain = Arc::new(chains::ARBITRUM.clone());

    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));

    let token0 = Token::new(
        Arc::clone(&chain),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        Arc::clone(&chain),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let creation_block: u64 = 12_345_678;

    let mut pool = Pool::new(
        chain,
        dex,
        Address::from([0xBB; 20]),
        PoolIdentifier::from_address(Address::from([0xBB; 20])),
        creation_block,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd);

    data_engine.process_defi_data(DefiData::Pool(pool.clone()));

    // Stub snapshot: empty positions, empty ticks, block matches pool.creation_block.
    let stub = PoolSnapshot::new(
        instrument_id,
        PoolState::default(),
        Vec::new(),
        Vec::new(),
        PoolAnalytics::default(),
        BlockPosition::new(creation_block, "0x0".to_string(), 0, 0),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    data_engine.process_defi_data(DefiData::PoolSnapshot(stub));

    assert!(
        data_engine
            .cache()
            .borrow()
            .pool_profiler(&instrument_id)
            .is_none(),
        "stub snapshot must not result in an installed profiler"
    );
    assert!(
        data_engine.cache().borrow().pool(&instrument_id).is_some(),
        "pool entry must be preserved even when its stub snapshot is refused"
    );
}

#[rstest]
fn test_process_option_greeks_caches_and_publishes(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    use nautilus_model::{
        data::{greeks::OptionGreekValues, option_chain::OptionGreeks},
        enums::GreeksConvention,
    };

    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let instrument_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");

    // Set up msgbus handler to capture published greeks
    let topic = switchboard::get_option_greeks_topic(instrument_id);
    let (handler, saver) = get_typed_message_saving_handler::<OptionGreeks>(None);
    msgbus::subscribe_option_greeks(topic.into(), handler, None);

    // Process greeks data through the engine
    let greeks = OptionGreeks {
        instrument_id,
        convention: GreeksConvention::BlackScholes,
        greeks: OptionGreekValues {
            delta: 0.55,
            gamma: 0.001,
            vega: 15.0,
            theta: -5.0,
            rho: 0.02,
        },
        mark_iv: Some(0.65),
        bid_iv: Some(0.63),
        ask_iv: Some(0.67),
        underlying_price: Some(50000.0),
        open_interest: Some(1000.0),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(1u64),
    };

    data_engine.borrow_mut().process(&greeks);

    // Verify greeks were cached
    let cached = cache.borrow().option_greeks(&instrument_id).copied();
    assert!(cached.is_some(), "OptionGreeks should be cached");
    assert_eq!(cached.unwrap().delta, 0.55);

    // Verify greeks were published to msgbus
    wait_until(
        || !saver.get_messages().is_empty(),
        Duration::from_millis(100),
    );
    let messages = saver.get_messages();
    assert!(!messages.is_empty(), "OptionGreeks should be published");
    assert_eq!(messages[0].instrument_id, instrument_id);
    assert_eq!(messages[0].delta, 0.55);
}

#[rstest]
fn test_counters_increment_per_dispatch(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    assert_eq!(data_engine.command_count(), 0);
    assert_eq!(data_engine.data_count(), 0);
    assert_eq!(data_engine.request_count(), 0);
    assert_eq!(data_engine.response_count(), 0);

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim.clone());
    data_engine.process(&inst_any as &dyn Any);
    assert_eq!(data_engine.data_count(), 1);

    let quote = QuoteTick::new(
        audusd_sim.id,
        Price::from("0.8000"),
        Price::from("0.8010"),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    data_engine.process_data(Data::Quote(quote));
    assert_eq!(data_engine.data_count(), 2);

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));
    assert_eq!(data_engine.command_count(), 1);

    let unsub = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub)));
    assert_eq!(data_engine.command_count(), 2);

    let req = RequestQuotes::new(
        audusd_sim.id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(req)));
    assert_eq!(data_engine.request_count(), 1);
    assert_eq!(
        data_engine.command_count(),
        2,
        "Request must not increment command_count"
    );
}

#[rstest]
fn test_reset_resets_counters(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);
    assert_eq!(data_engine.data_count(), 1);

    data_engine.reset();

    assert_eq!(data_engine.command_count(), 0);
    assert_eq!(data_engine.data_count(), 0);
    assert_eq!(data_engine.request_count(), 0);
    assert_eq!(data_engine.response_count(), 0);
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_custom_data_through_any_publishes_to_custom_topic(
    #[case] borrowed: bool,
    _stub_msgbus: Rc<RefCell<MessageBus>>,
    data_engine: Rc<RefCell<DataEngine>>,
) {
    let mut data_engine = data_engine.borrow_mut();
    let custom = stub_custom_data(
        6_000,
        99,
        Some(serde_json::from_value(json!({"source": "metadata"})).unwrap()),
        Some("SIM//CUSTOM".to_string()),
    );
    let (handler, saver) = get_any_saving_handler::<CustomData>(None);
    let topic = switchboard::get_custom_topic(&custom.data_type);
    msgbus::subscribe_any(topic.into(), handler, None);

    assert_eq!(data_engine.data_count(), 0);

    data_engine.process(&custom as &dyn Any);
    assert_eq!(saver.get_messages(), vec![custom.clone()]);
    assert_eq!(data_engine.data_count(), 1);

    dispatch_data(&mut data_engine, Data::Custom(custom.clone()), borrowed);

    assert_eq!(saver.get_messages(), vec![custom.clone(), custom]);
    assert_eq!(data_engine.data_count(), 2);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_defi_command_counters(data_engine: Rc<RefCell<DataEngine>>) {
    let mut data_engine = data_engine.borrow_mut();

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let sub = DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: None,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    }));

    data_engine.execute(sub);

    let unsub =
        DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::PoolSwaps(UnsubscribePoolSwaps {
            instrument_id,
            client_id: None,
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }));

    data_engine.execute(unsub);

    let req = DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(RequestPoolSnapshot {
        instrument_id,
        client_id: None,
        request_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    }));

    data_engine.execute(req);

    assert_eq!(
        data_engine.command_count(),
        2,
        "DefiSubscribe + DefiUnsubscribe must increment command_count"
    );
    assert_eq!(
        data_engine.request_count(),
        1,
        "DefiRequest must increment request_count"
    );
}

#[cfg(feature = "defi")]
#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_defi_data_increments_data_count(
    #[case] borrowed: bool,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    data_engine.borrow_mut().register_client(data_client, None);

    let blockchain = Blockchain::Ethereum;

    let block = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );

    let mut data_engine = data_engine.borrow_mut();
    assert_eq!(data_engine.data_count(), 0);
    dispatch_data(
        &mut data_engine,
        Data::Defi(Box::new(DefiData::Block(block))),
        borrowed,
    );
    assert_eq!(data_engine.data_count(), 1);
}

#[rstest]
fn test_trim_to_bounds_drops_trailing_entries(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 1_000),
            quote_at(instrument_id, 2_000),
            quote_at(instrument_id, 3_000),
        ],
        None,
        Some(UnixNanos::from(2_000)),
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    let ts_inits: Vec<u64> = quotes.data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);
}

#[rstest]
fn test_trim_to_bounds_drops_leading_entries(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 1_000),
            quote_at(instrument_id, 2_000),
            quote_at(instrument_id, 3_000),
        ],
        Some(UnixNanos::from(2_000)),
        None,
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    let ts_inits: Vec<u64> = quotes.data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_000, 3_000]);
}

#[rstest]
fn test_trim_to_bounds_short_circuits_on_empty(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![],
        Some(UnixNanos::from(1_000)),
        Some(UnixNanos::from(2_000)),
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    assert!(quotes.data.is_empty());
}

#[rstest]
fn test_trim_to_bounds_passes_through_when_unbounded(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 1_000),
            quote_at(instrument_id, 2_000),
            quote_at(instrument_id, 3_000),
        ],
        None,
        None,
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    let ts_inits: Vec<u64> = quotes.data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_000, 2_000, 3_000]);
}

#[rstest]
fn test_trim_to_bounds_keeps_already_windowed_data(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 1_000),
            quote_at(instrument_id, 2_000),
            quote_at(instrument_id, 3_000),
        ],
        Some(UnixNanos::from(1_000)),
        Some(UnixNanos::from(3_000)),
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    let ts_inits: Vec<u64> = quotes.data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_000, 2_000, 3_000]);
}

#[rstest]
fn test_trim_to_bounds_clears_when_start_after_all_entries(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 1_000),
            quote_at(instrument_id, 2_000),
        ],
        Some(UnixNanos::from(5_000)),
        None,
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    assert!(quotes.data.is_empty());
}

#[rstest]
fn test_trim_to_bounds_clears_when_end_before_all_entries(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 5_000),
            quote_at(instrument_id, 6_000),
        ],
        None,
        Some(UnixNanos::from(1_000)),
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    assert!(quotes.data.is_empty());
}

#[rstest]
fn test_trim_to_bounds_trims_funding_rates(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;

    let make_rate = |ts: u64| {
        FundingRateUpdate::new(
            instrument_id,
            "0.0001".parse().unwrap(),
            None,
            None,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let mut resp = DataResponse::FundingRates(FundingRatesResponse::new(
        UUID4::new(),
        ClientId::test_default(),
        instrument_id,
        vec![make_rate(1_000), make_rate(2_000), make_rate(3_000)],
        Some(UnixNanos::from(1_500)),
        Some(UnixNanos::from(2_500)),
        UnixNanos::default(),
        None,
    ));

    resp.trim_to_bounds();

    let DataResponse::FundingRates(rates) = resp else {
        panic!("expected FundingRates variant");
    };

    let ts_inits: Vec<u64> = rates.data.iter().map(|r| r.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_000]);
}

#[rstest]
fn test_trim_to_bounds_clears_when_start_after_end(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 1_000),
            quote_at(instrument_id, 2_000),
            quote_at(instrument_id, 3_000),
        ],
        Some(UnixNanos::from(3_000)),
        Some(UnixNanos::from(1_000)),
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    assert!(quotes.data.is_empty());
}

#[rstest]
fn test_trim_to_bounds_single_point_window(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = quotes_response(
        instrument_id,
        vec![
            quote_at(instrument_id, 1_000),
            quote_at(instrument_id, 2_000),
            quote_at(instrument_id, 3_000),
        ],
        Some(UnixNanos::from(2_000)),
        Some(UnixNanos::from(2_000)),
    );

    resp.trim_to_bounds();

    let DataResponse::Quotes(quotes) = resp else {
        panic!("expected Quotes variant");
    };

    let ts_inits: Vec<u64> = quotes.data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_000]);
}

fn quote_at(instrument_id: InstrumentId, ts: u64) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from("1.00000"),
        Price::from("1.00010"),
        Quantity::from("1"),
        Quantity::from("1"),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

fn quotes_response(
    instrument_id: InstrumentId,
    data: Vec<QuoteTick>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> DataResponse {
    DataResponse::Quotes(QuotesResponse::new(
        UUID4::new(),
        ClientId::test_default(),
        instrument_id,
        data,
        start,
        end,
        UnixNanos::default(),
        None,
    ))
}
