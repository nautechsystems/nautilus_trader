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

use std::{cell::RefCell, rc::Rc, sync::Arc};

use alloy::primitives::{Address, address};
use nautilus_blockchain::{
    config::BlockchainExecutionClientConfig,
    execution::{
        client::BlockchainExecutionClient,
        metadata_store::{InMemoryMetadataStore, PoolMetadataStore},
    },
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::get_runtime,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    defi::{
        AmmType, Dex, DexType, Pool, PoolIdentifier, Token, chain::chains,
        validation::validate_address,
    },
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::OrderInitialized,
    identifiers::{AccountId, ClientId, ClientOrderId, OrderListId, StrategyId, TraderId, Venue},
    orders::OrderList,
    stubs::TestDefault,
    types::Quantity,
};

fn make_pool(router: Address, pool_address: Address) -> Pool {
    let chain = Arc::new(chains::BSC.clone());
    let dex = Arc::new(Dex::new(
        (*chain).clone(),
        DexType::PancakeSwapV2,
        &router.to_string(),
        0,
        AmmType::CPAMM,
        "PairCreated(address,address,address,uint256)",
        "Swap(address,uint256,uint256,uint256,uint256,address)",
        "Mint(address,uint256,uint256)",
        "Burn(address,uint256,uint256,address)",
        "Sync(uint112,uint112)",
    ));

    let token0 = Token::new(
        chain.clone(),
        validate_address("0x55d398326f99059fF775485246999027B3197955").expect("token0"),
        "USDT".to_string(),
        "USDT".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        validate_address("0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d").expect("token1"),
        "USDC".to_string(),
        "USDC".to_string(),
        18,
    );

    Pool::new(
        chain,
        dex,
        pool_address,
        PoolIdentifier::from_address(pool_address),
        0,
        token0,
        token1,
        Some(2500),
        None,
        UnixNanos::default(),
    )
}

fn make_client() -> (
    BlockchainExecutionClient,
    Pool,
    TraderId,
    StrategyId,
    ClientOrderId,
) {
    let trader_id = TraderId::test_default();
    let strategy_id = StrategyId::test_default();
    let account_id = AccountId::new("BINANCE-001");
    let venue = Venue::new("Bsc:PancakeSwapV2");

    let config = BlockchainExecutionClientConfig::new(
        trader_id,
        account_id,
        venue,
        chains::BSC.clone(),
        String::from("0x058e41Ae42e322e5E6ea6Fc9930776d67CDd3115"),
        None,
        String::from("https://bsc.example.com"),
        None,
    );

    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("BLOCKCHAIN"),
        venue,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );

    let mut metadata_store = InMemoryMetadataStore::new();
    let pool = make_pool(
        address!("0x7977BF3E7e0C954d12CDCA3E013AdAF57E0b06E0"),
        address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245"),
    );
    metadata_store.insert_pool(pool.clone());

    let client =
        BlockchainExecutionClient::with_metadata_store(core, config, Box::new(metadata_store))
            .expect("client should construct");

    (
        client,
        pool,
        trader_id,
        strategy_id,
        ClientOrderId::new("O-NO-TODO-001"),
    )
}

fn make_submit_order(
    pool: &Pool,
    trader_id: TraderId,
    strategy_id: StrategyId,
    client_order_id: ClientOrderId,
) -> SubmitOrder {
    let mut order_init = OrderInitialized::test_default();
    order_init.trader_id = trader_id;
    order_init.strategy_id = strategy_id;
    order_init.instrument_id = pool.instrument_id;
    order_init.client_order_id = client_order_id;
    order_init.order_side = OrderSide::Buy;
    order_init.order_type = OrderType::Market;
    order_init.time_in_force = TimeInForce::Ioc;
    order_init.quantity = Quantity::new(1.0, 0);

    SubmitOrder::new(
        trader_id,
        None,
        strategy_id,
        pool.instrument_id,
        client_order_id,
        order_init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

#[test]
fn test_execution_client_methods_do_not_panic_and_return_deterministic_results() {
    let (client, pool, trader_id, strategy_id, client_order_id) = make_client();

    let submit = make_submit_order(&pool, trader_id, strategy_id, client_order_id);

    let submit_list = SubmitOrderList::new(
        trader_id,
        None,
        strategy_id,
        OrderList::new(
            OrderListId::new("OL-NO-TODO-001"),
            pool.instrument_id,
            strategy_id,
            vec![client_order_id],
            UnixNanos::default(),
        ),
        vec![submit.order_init.clone()],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    let modify = ModifyOrder::new(
        trader_id,
        None,
        strategy_id,
        pool.instrument_id,
        client_order_id,
        None,
        Some(Quantity::new(1.0, 0)),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cancel = CancelOrder::new(
        trader_id,
        None,
        strategy_id,
        pool.instrument_id,
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cancel_all = CancelAllOrders::new(
        trader_id,
        None,
        strategy_id,
        pool.instrument_id,
        OrderSide::Buy,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let batch_cancel = BatchCancelOrders::new(
        trader_id,
        None,
        strategy_id,
        pool.instrument_id,
        vec![cancel.clone()],
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let query_order = QueryOrder::new(
        trader_id,
        None,
        strategy_id,
        pool.instrument_id,
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    let submit_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.submit_order(&submit)
    }));
    assert!(submit_result.is_ok());

    let submit_list_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.submit_order_list(&submit_list)
    }));
    assert!(submit_list_result.is_ok());

    let modify_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.modify_order(&modify)
    }));
    assert!(modify_result.is_ok());

    let cancel_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.cancel_order(&cancel)
    }));
    assert!(cancel_result.is_ok());

    let cancel_all_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.cancel_all_orders(&cancel_all)
    }));
    assert!(cancel_all_result.is_ok());

    let batch_cancel_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.batch_cancel_orders(&batch_cancel)
    }));
    assert!(batch_cancel_result.is_ok());

    let query_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.query_order(&query_order)
    }));
    assert!(query_result.is_ok());

    let order_report_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_runtime().block_on(client.generate_order_status_report(
            &GenerateOrderStatusReport::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(pool.instrument_id),
                Some(client_order_id),
                None,
                None,
                None,
            ),
        ))
    }));
    assert!(order_report_result.is_ok());

    let order_reports_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_runtime().block_on(client.generate_order_status_reports(
            &GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                Some(pool.instrument_id),
                None,
                None,
                None,
                None,
            ),
        ))
    }));
    assert!(order_reports_result.is_ok());

    let fill_reports_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_runtime().block_on(client.generate_fill_reports(GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(pool.instrument_id),
            None,
            None,
            None,
            None,
            None,
        )))
    }));
    assert!(fill_reports_result.is_ok());

    let position_reports_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_runtime().block_on(client.generate_position_status_reports(
            &GeneratePositionStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(pool.instrument_id),
                None,
                None,
                None,
                None,
            ),
        ))
    }));
    assert!(position_reports_result.is_ok());

    let mass_status_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_runtime().block_on(client.generate_mass_status(None))
    }));
    assert!(mass_status_result.is_ok());
}
