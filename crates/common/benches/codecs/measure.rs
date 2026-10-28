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

use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::VecDeque,
    fs,
    hint::black_box,
    path::Path,
    rc::Rc,
    time::Instant,
};

use bytes::Bytes;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarType, CustomData, Data, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate,
        OptionGreekValues, OptionGreeks, OrderBookDeltas, OrderBookDepth, QuoteTick, TradeTick,
        option_chain::StrikeRange,
        stubs::{
            quote_ethusdt_binance, stub_bar, stub_custom_data, stub_delta, stub_deltas,
            stub_depth10, stub_instrument_close, stub_instrument_status, stub_trade_ethusdt_buy,
        },
    },
    enums::{
        AccountType, BookType, LiquiditySide, OrderSide, OrderStatus, OrderType,
        PositionAdjustmentType, PositionSide, TimeInForce,
    },
    events::{
        AccountState, OrderEventAny, OrderInitialized, PortfolioSnapshot, PositionAdjusted,
        PositionChanged, PositionClosed, PositionEvent, PositionOpened, order::spec::*,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::*},
    orders::OrderList,
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::rstest;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use super::{
    deserialize_payload,
    native_tests::{params, responses},
    serialize_payload,
};
use crate::{
    enums::SerializationEncoding,
    messages::{
        data::{
            DataResponse, RequestBars, RequestBookDeltas, RequestBookDepth, RequestBookSnapshot,
            RequestCommand, RequestCustomData, RequestFundingRates, RequestInstrument,
            RequestInstruments, RequestJoin, RequestOptionChainReferencePrice, RequestQuotes,
            RequestTrades, SubscribeCommand, UnsubscribeCommand, subscribe::*,
        },
        execution::{
            BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder,
            GenerateExecutionMassStatus, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
            SubmitOrderList, TradingCommand,
        },
    },
    msgbus::{
        self, BusMessage, BusPayloadType, MStr, MessageBus, MessageBusConfig,
        MessageBusExternalEgress, ShareableMessageHandler, Topic, TypedHandler, set_message_bus,
    },
};

const TOPIC: &str = "benchmark.native";
const CAPACITY: usize = 128;
const WARMUP: usize = 1_000;
const TS_BASE: u64 = 1_700_000_000_000_000_000;

#[rstest]
#[ignore = "optimized message codec and delivery measurements"]
fn native_codec_measurements() {
    let samples = setting("NAUTILUS_BENCH_SAMPLES", 100_000);
    let trials = setting("NAUTILUS_BENCH_TRIALS", 3);
    let warmup = setting("NAUTILUS_BENCH_WARMUP", WARMUP);
    let selected_case = std::env::var("NAUTILUS_BENCH_CASE").ok();
    let selected_format = std::env::var("NAUTILUS_BENCH_FORMAT").ok();
    let profile = std::env::var("NAUTILUS_BENCH_PROFILE").ok();
    let formats = [
        ("json", SerializationEncoding::Json),
        ("msgpack", SerializationEncoding::MsgPack),
    ];
    println!(
        "{}",
        json!({"environment": {"samples": samples, "trials": trials, "warmup": warmup, "queue_capacity": CAPACITY, "profile": profile}})
    );

    for trial in 0..trials {
        for index in 0..formats.len() {
            let (format, encoding) = formats[(index + trial) % formats.len()];

            if selected_format
                .as_deref()
                .is_some_and(|value| value != format)
            {
                continue;
            }

            let config = Run {
                samples,
                warmup,
                trial,
                format,
                encoding,
                profile: profile.as_deref(),
                selected_case: selected_case.as_deref(),
            };

            measure_format(&config);
        }
    }

    reset_bus();
}

fn measure_format(config: &Run<'_>) {
    let mut init = OrderInitializedSpec::builder()
        .ts_event((TS_BASE + 123).into())
        .ts_init((TS_BASE + 124).into())
        .event_id(UUID4::new())
        .build();
    init.causation_id = Some(UUID4::new());

    let submit = TradingCommand::SubmitOrder(SubmitOrder {
        trader_id: init.trader_id,
        client_id: Some(ClientId::from("CLIENT-1")),
        strategy_id: init.strategy_id,
        instrument_id: init.instrument_id,
        client_order_id: init.client_order_id,
        order_init: init,
        exec_algorithm_id: None,
        position_id: None,
        params: Some(params()),
        command_id: UUID4::new(),
        ts_init: (TS_BASE + 127).into(),
        correlation_id: Some(UUID4::new()),
        causation_id: Some(UUID4::new()),
    });

    let scalar_params = nautilus_core::Params::new(
        [
            ("post_only".into(), Value::Bool(true)),
            ("route".into(), Value::from("external")),
        ]
        .into(),
    );

    let cancel = TradingCommand::CancelOrder(CancelOrder {
        trader_id: TraderId::from("TRADER-1"),
        client_id: Some(ClientId::from("CLIENT-1")),
        strategy_id: StrategyId::from("STRATEGY-1"),
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        client_order_id: ClientOrderId::from("ORDER-1"),
        venue_order_id: Some(VenueOrderId::from("VENUE-2")),
        command_id: UUID4::new(),
        ts_init: (TS_BASE + 128).into(),
        params: Some(scalar_params),
        correlation_id: Some(UUID4::new()),
        causation_id: Some(UUID4::new()),
    });

    let mut accepted = OrderAcceptedSpec::builder()
        .event_id(UUID4::new())
        .ts_event(UnixNanos::from(TS_BASE + 129))
        .ts_init(UnixNanos::from(TS_BASE + 130))
        .build();
    accepted.causation_id = Some(UUID4::new());
    let accepted = OrderEventAny::Accepted(accepted);

    let modify = TradingCommand::ModifyOrder(ModifyOrder {
        trader_id: TraderId::from("TRADER-1"),
        client_id: Some(ClientId::from("CLIENT-1")),
        strategy_id: StrategyId::from("STRATEGY-1"),
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        client_order_id: ClientOrderId::from("ORDER-1"),
        venue_order_id: Some(VenueOrderId::from("VENUE-2")),
        quantity: Some(Quantity::from("7.500")),
        price: Some(Price::from("20001.125")),
        trigger_price: Some(Price::from("19999.750")),
        command_id: UUID4::new(),
        ts_init: (TS_BASE + 137).into(),
        params: Some(params()),
        correlation_id: Some(UUID4::new()),
        causation_id: Some(UUID4::new()),
    });

    let query = TradingCommand::QueryAccount(QueryAccount::new(
        TraderId::from("TRADER-1"),
        Some(ClientId::from("CLIENT-1")),
        AccountId::from("SIM-1"),
        UUID4::new(),
        (TS_BASE + 138).into(),
        None,
        None,
    ));
    let mut updated = OrderUpdatedSpec::builder()
        .event_id(UUID4::new())
        .ts_event((TS_BASE + 139).into())
        .ts_init((TS_BASE + 140).into())
        .build();
    updated.causation_id = Some(UUID4::new());
    let updated = OrderEventAny::Updated(updated);
    let mut filled = OrderFilledSpec::builder()
        .event_id(UUID4::new())
        .ts_event((TS_BASE + 141).into())
        .ts_init((TS_BASE + 142).into())
        .build();
    filled.causation_id = Some(UUID4::new());
    filled.last_qty = Quantity::from("2.750");
    filled.last_px = Price::from("20001.125");
    filled.commission = Some(Money::from("0.73 USD"));
    let filled = OrderEventAny::Filled(filled);
    let instrument = responses(1)
        .into_iter()
        .find(|value| matches!(value, DataResponse::Instrument(_)))
        .unwrap();
    let mut catalog = responses(1)
        .into_iter()
        .find(|value| matches!(value, DataResponse::Instruments(_)))
        .unwrap();

    if let DataResponse::Instruments(value) = &mut catalog {
        value.data.resize(8, value.data[0].clone());
    }

    let request = RequestCommand::Join(RequestJoin {
        request_ids: vec![UUID4::new(), UUID4::new()],
        start: Some(jiff::Timestamp::new(-91, -123_456_789).unwrap()),
        end: Some(jiff::Timestamp::new(92, 987_654_321).unwrap()),
        request_id: UUID4::new(),
        ts_init: (TS_BASE + 131).into(),
        params: Some(params()),
        correlation_id: Some(UUID4::new()),
    });

    let mut reply = responses(1)
        .into_iter()
        .find(|value| matches!(value, DataResponse::Trades(_)))
        .unwrap();

    if let DataResponse::Trades(value) = &mut reply {
        value.instrument_ids = vec![value.data[0].instrument_id];
        let first = value.data[0];
        let mut rows = value.data.to_vec();
        rows.resize(32, first);
        value.data = rows.into();
    }

    let custom = responses(1).remove(1);

    let mut account = AccountState::new(
        AccountId::from("SIM-1"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000.75 USD"),
            Money::from("250.25 USD"),
            Money::from("750.50 USD"),
        )],
        vec![],
        true,
        UUID4::new(),
        (TS_BASE + 132).into(),
        (TS_BASE + 133).into(),
        Some(Currency::USD()),
    );
    account.info = Some(params());
    let mut account_plain = account.clone();
    account_plain.info = None;

    let mut report = OrderStatusReport::new(
        AccountId::from("SIM-1"),
        InstrumentId::from("BTCUSDT.BINANCE"),
        Some(ClientOrderId::from("ORDER-1")),
        VenueOrderId::from("VENUE-2"),
        Some(OrderSide::Buy),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::PartiallyFilled,
        Quantity::from("3.750"),
        Quantity::from("1.125"),
        (TS_BASE + 134).into(),
        (TS_BASE + 135).into(),
        (TS_BASE + 136).into(),
        Some(UUID4::new()),
    );
    report.price = Some(Price::from("20001.125"));
    report.avg_px = Some(rust_decimal_macros::dec!(20000.123456789));
    let commands = execution_commands(&submit, &cancel, &modify);
    let events = execution_events();
    let positions = execution_positions();
    let response_cases = data_responses();
    let request_cases = data_requests();
    let subscription_cases = data_subscriptions();
    let (fill_report, position_report) = execution_reports();

    let mut mass_reports = Vec::new();

    for count in [0, 1, 32, 256] {
        let mut mass = ExecutionMassStatus::new(
            ClientId::from("CLIENT-1"),
            AccountId::from("SIM-1"),
            "BINANCE".into(),
            (TS_BASE + 220).into(),
            Some(UUID4::new()),
        );
        mass.set_report_window(Some((TS_BASE - 1_000).into()), true);
        let mut orders = Vec::new();

        for index in 0..count {
            let mut order = report.clone();
            order.venue_order_id = VenueOrderId::new(format!("VENUE-{index}"));
            orders.push(order);
        }

        mass.add_order_reports(orders);
        mass.add_fill_reports(vec![fill_report.clone(); count]);
        mass.add_position_reports(vec![position_report.clone(); count]);
        mass_reports.push((format!("execution_mass_status_{count}"), mass));
    }

    let snapshot = PortfolioSnapshot::new(
        account.account_id,
        account.account_type,
        account.base_currency,
        account.balances.clone(),
        account.margins.clone(),
        vec![Money::from("12.34 USD")],
        vec![Money::from("-5.67 USD")],
        vec![Money::from("1012.34 USD")],
        Some(Money::from("1012.34 USD")),
        true,
        vec![InstrumentId::from("BTCUSDT.BINANCE")],
        vec![Currency::USDT()],
        vec![InstrumentId::from("ETHUSDT.BINANCE")],
        UUID4::new(),
        (TS_BASE + 221).into(),
        (TS_BASE + 222).into(),
    );

    let mass_request = GenerateExecutionMassStatus {
        trader_id: TraderId::from("TRADER-1"),
        client_id: ClientId::from("CLIENT-1"),
        venue: None,
        command_id: UUID4::new(),
        ts_init: (TS_BASE + 141).into(),
        params: Some(params()),
        correlation_id: Some(UUID4::new()),
        causation_id: Some(UUID4::new()),
    };

    market_data(config);
    report_bundles(&report, &fill_report, config);

    for (name, value) in &subscription_cases {
        run(
            &format!("subscribe_{name}"),
            BusPayloadType::SubscribeCommand,
            value,
            |topic, value| msgbus::publish_any(topic, value),
            install_any::<SubscribeCommand>,
            config,
        );
        let unsubscribe = value.clone().into_unsubscribe(
            UUID4::new(),
            (TS_BASE + 311).into(),
            Some(UUID4::new()),
        );
        run(
            &format!("unsubscribe_{name}"),
            BusPayloadType::UnsubscribeCommand,
            &unsubscribe,
            |topic, value| msgbus::publish_any(topic, value),
            install_any::<UnsubscribeCommand>,
            config,
        );
    }

    measure_command("submit_order", &submit, config);
    measure_command("cancel_order", &cancel, config);
    measure_command("modify_order", &modify, config);
    measure_command("query_account", &query, config);
    run(
        "order_updated",
        BusPayloadType::OrderEvent,
        &updated,
        msgbus::publish_order_event,
        install_order,
        config,
    );
    run(
        "order_filled",
        BusPayloadType::OrderEvent,
        &filled,
        msgbus::publish_order_event,
        install_order,
        config,
    );
    run(
        "instrument_reply",
        BusPayloadType::DataResponse,
        &instrument,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<DataResponse>,
        config,
    );
    run(
        "catalog_reply",
        BusPayloadType::DataResponse,
        &catalog,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<DataResponse>,
        config,
    );
    run(
        "order_accepted",
        BusPayloadType::OrderEvent,
        &accepted,
        msgbus::publish_order_event,
        install_order,
        config,
    );
    run(
        "order_status_report",
        BusPayloadType::OrderStatusReport,
        &report,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<OrderStatusReport>,
        config,
    );
    run(
        "request_join",
        BusPayloadType::RequestCommand,
        &request,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<RequestCommand>,
        config,
    );
    run(
        "historical_trades",
        BusPayloadType::DataResponse,
        &reply,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<DataResponse>,
        config,
    );
    run(
        "custom_batch",
        BusPayloadType::DataResponse,
        &custom,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<DataResponse>,
        config,
    );

    for (name, value) in &mass_reports {
        run(
            name,
            BusPayloadType::ExecutionMassStatus,
            value,
            |topic, value| msgbus::publish_any(topic, value),
            install_any::<ExecutionMassStatus>,
            config,
        );
    }

    run(
        "portfolio_snapshot",
        BusPayloadType::PortfolioSnapshot,
        &snapshot,
        msgbus::publish_portfolio_snapshot,
        install_snapshot,
        config,
    );

    for (name, value) in &response_cases {
        run(
            name,
            BusPayloadType::DataResponse,
            value,
            |topic, value| msgbus::publish_any(topic, value),
            install_any::<DataResponse>,
            config,
        );
    }

    for (name, value) in &request_cases {
        run(
            name,
            BusPayloadType::RequestCommand,
            value,
            |topic, value| msgbus::publish_any(topic, value),
            install_any::<RequestCommand>,
            config,
        );
    }

    run(
        "account_state",
        BusPayloadType::AccountState,
        &account,
        msgbus::publish_account_state,
        install_account,
        config,
    );

    run(
        "account_state_plain",
        BusPayloadType::AccountState,
        &account_plain,
        msgbus::publish_account_state,
        install_account,
        config,
    );

    for (name, value) in &commands {
        measure_command(name, value, config);
    }

    for (name, value) in &events {
        run(
            name,
            BusPayloadType::OrderEvent,
            value,
            msgbus::publish_order_event,
            install_order,
            config,
        );
    }

    for (name, value) in &positions {
        run(
            name,
            BusPayloadType::PositionEvent,
            value,
            msgbus::publish_position_event,
            install_position,
            config,
        );
    }

    run(
        "fill_report",
        BusPayloadType::FillReport,
        &fill_report,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<FillReport>,
        config,
    );
    run(
        "position_status_report",
        BusPayloadType::PositionStatusReport,
        &position_report,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<PositionStatusReport>,
        config,
    );
    run(
        "generate_mass_status",
        BusPayloadType::GenerateExecutionMassStatus,
        &mass_request,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<GenerateExecutionMassStatus>,
        config,
    );
}

#[derive(Serialize, serde::Deserialize)]
struct OrderWithFills {
    order_report: OrderStatusReport,
    fill_reports: Vec<FillReport>,
}

fn report_bundles(order: &OrderStatusReport, fill: &FillReport, config: &Run<'_>) {
    if !matches!(
        config.profile,
        Some("decode_latency" | "codec_latency" | "encode" | "decode")
    ) {
        return;
    }

    for count in [0, 1, 32, 256] {
        let value = OrderWithFills {
            order_report: order.clone(),
            fill_reports: vec![fill.clone(); count],
        };

        run(
            &format!("generic_report_order_with_fills_{count}"),
            BusPayloadType::ExecutionMassStatus,
            &value,
            |_, _| unreachable!(),
            |_| unreachable!(),
            config,
        );
    }
}

fn measure_command(name: &str, command: &TradingCommand, config: &Run<'_>) {
    run(
        name,
        BusPayloadType::TradingCommand,
        command,
        |topic, value| msgbus::publish_any(topic, value),
        install_any::<TradingCommand>,
        config,
    );

    if matches!(
        config.profile,
        Some("decode_latency" | "codec_latency" | "encode" | "decode")
    ) {
        execution_concrete(name, command, config);
    }
}

fn execution_commands(
    submit: &TradingCommand,
    cancel: &TradingCommand,
    modify: &TradingCommand,
) -> [(&'static str, TradingCommand); 12] {
    let TradingCommand::SubmitOrder(submit) = submit else {
        unreachable!()
    };

    let TradingCommand::CancelOrder(cancel) = cancel else {
        unreachable!()
    };

    let TradingCommand::ModifyOrder(modify) = modify else {
        unreachable!()
    };

    let mut limit = submit.clone();
    limit.order_init.order_type = OrderType::Limit;
    limit.order_init.price = Some(Price::from("20001.125"));
    limit.order_init.quantity = Quantity::from("3.750");
    limit.order_init.client_order_id = ClientOrderId::from("ORDER-LIMIT-1");
    limit.client_order_id = limit.order_init.client_order_id;
    let mut submit_plain = submit.clone();
    submit_plain.params = None;
    let mut limit_plain = limit.clone();
    limit_plain.params = None;
    let mut modify_plain = modify.clone();
    modify_plain.params = None;
    let mut cancel_plain = cancel.clone();
    cancel_plain.params = None;
    let mut init_second = submit.order_init.clone();
    init_second.client_order_id = ClientOrderId::from("ORDER-2");
    init_second.event_id = UUID4::new();
    let mut cancel_second = cancel.clone();
    cancel_second.client_order_id = init_second.client_order_id;
    cancel_second.venue_order_id = Some(VenueOrderId::from("VENUE-3"));
    let mut modify_second = modify.clone();
    modify_second.client_order_id = init_second.client_order_id;
    modify_second.venue_order_id = cancel_second.venue_order_id;
    modify_second.quantity = Some(Quantity::from("8.250"));

    let order_list = OrderList::new(
        OrderListId::from("LIST-1"),
        submit.instrument_id,
        submit.strategy_id,
        vec![submit.client_order_id, init_second.client_order_id],
        (TS_BASE + 142).into(),
    );

    let list = SubmitOrderList {
        trader_id: submit.trader_id,
        client_id: submit.client_id,
        strategy_id: submit.strategy_id,
        instrument_id: submit.instrument_id,
        order_list,
        order_inits: vec![submit.order_init.clone(), init_second],
        exec_algorithm_id: None,
        position_id: None,
        params: Some(params()),
        command_id: UUID4::new(),
        ts_init: (TS_BASE + 143).into(),
        correlation_id: Some(UUID4::new()),
        causation_id: Some(UUID4::new()),
    };

    let mut algorithm = submit.clone();
    algorithm.exec_algorithm_id = Some(ExecAlgorithmId::from("TWAP"));
    algorithm.position_id = Some(PositionId::from("POSITION-1"));
    execution_algorithm(&mut algorithm.order_init);
    let mut list_algorithm = list.clone();
    list_algorithm.exec_algorithm_id = Some(ExecAlgorithmId::from("TWAP"));
    list_algorithm.position_id = Some(PositionId::from("POSITION-2"));
    for init in &mut list_algorithm.order_inits {
        execution_algorithm(init);
    }

    [
        (
            "submit_order_plain",
            TradingCommand::SubmitOrder(submit_plain),
        ),
        (
            "submit_order_limit_plain",
            TradingCommand::SubmitOrder(limit_plain),
        ),
        (
            "modify_order_plain",
            TradingCommand::ModifyOrder(modify_plain),
        ),
        (
            "cancel_order_plain",
            TradingCommand::CancelOrder(cancel_plain),
        ),
        ("submit_order_limit", TradingCommand::SubmitOrder(limit)),
        (
            "submit_order_algorithm",
            TradingCommand::SubmitOrder(algorithm),
        ),
        (
            "submit_order_list_algorithm",
            TradingCommand::SubmitOrderList(list_algorithm),
        ),
        ("submit_order_list", TradingCommand::SubmitOrderList(list)),
        (
            "modify_orders",
            TradingCommand::ModifyOrders(BatchModifyOrders {
                trader_id: modify.trader_id,
                client_id: modify.client_id,
                strategy_id: modify.strategy_id,
                instrument_id: modify.instrument_id,
                modifies: vec![modify.clone(), modify_second],
                command_id: UUID4::new(),
                ts_init: (TS_BASE + 144).into(),
                params: Some(params()),
                correlation_id: Some(UUID4::new()),
                causation_id: Some(UUID4::new()),
            }),
        ),
        (
            "cancel_orders",
            TradingCommand::CancelOrders(BatchCancelOrders {
                trader_id: cancel.trader_id,
                client_id: cancel.client_id,
                strategy_id: cancel.strategy_id,
                instrument_id: cancel.instrument_id,
                cancels: vec![cancel.clone(), cancel_second],
                command_id: UUID4::new(),
                ts_init: (TS_BASE + 145).into(),
                params: Some(params()),
                correlation_id: Some(UUID4::new()),
                causation_id: Some(UUID4::new()),
            }),
        ),
        (
            "cancel_all_orders",
            TradingCommand::CancelAllOrders(CancelAllOrders {
                trader_id: cancel.trader_id,
                client_id: cancel.client_id,
                strategy_id: cancel.strategy_id,
                instrument_id: cancel.instrument_id,
                order_side: Some(OrderSide::Buy),
                command_id: UUID4::new(),
                ts_init: (TS_BASE + 146).into(),
                params: Some(params()),
                correlation_id: Some(UUID4::new()),
                causation_id: Some(UUID4::new()),
            }),
        ),
        (
            "query_order",
            TradingCommand::QueryOrder(QueryOrder {
                trader_id: cancel.trader_id,
                client_id: cancel.client_id,
                strategy_id: cancel.strategy_id,
                instrument_id: cancel.instrument_id,
                client_order_id: cancel.client_order_id,
                venue_order_id: cancel.venue_order_id,
                command_id: UUID4::new(),
                ts_init: (TS_BASE + 147).into(),
                params: Some(params()),
                correlation_id: Some(UUID4::new()),
                causation_id: Some(UUID4::new()),
            }),
        ),
    ]
}

fn execution_algorithm(init: &mut OrderInitialized) {
    init.exec_algorithm_id = Some(ExecAlgorithmId::from("TWAP"));
    init.exec_spawn_id = Some(ClientOrderId::from("SPAWN-1"));
    init.exec_algorithm_params = Some(
        [
            ("zeta".into(), "last".into()),
            ("alpha".into(), "first".into()),
        ]
        .into_iter()
        .collect(),
    );
}

fn execution_events() -> [(&'static str, OrderEventAny); 14] {
    macro_rules! event {
        ($name:literal, $variant:ident, $spec:ty) => {{
            let mut value = <$spec>::builder()
                .event_id(UUID4::new())
                .ts_event((TS_BASE + 148).into())
                .ts_init((TS_BASE + 149).into())
                .build();
            value.causation_id = Some(UUID4::new());
            ($name, OrderEventAny::$variant(value))
        }};
    }
    [
        event!("order_initialized", Initialized, OrderInitializedSpec),
        event!("order_denied", Denied, OrderDeniedSpec),
        event!("order_emulated", Emulated, OrderEmulatedSpec),
        event!("order_released", Released, OrderReleasedSpec),
        event!("order_submitted", Submitted, OrderSubmittedSpec),
        event!("order_rejected", Rejected, OrderRejectedSpec),
        event!("order_canceled", Canceled, OrderCanceledSpec),
        event!("order_expired", Expired, OrderExpiredSpec),
        event!("order_triggered", Triggered, OrderTriggeredSpec),
        event!(
            "order_pending_update",
            PendingUpdate,
            OrderPendingUpdateSpec
        ),
        event!(
            "order_pending_cancel",
            PendingCancel,
            OrderPendingCancelSpec
        ),
        event!(
            "order_modify_rejected",
            ModifyRejected,
            OrderModifyRejectedSpec
        ),
        event!(
            "order_cancel_rejected",
            CancelRejected,
            OrderCancelRejectedSpec
        ),
        event!("order_fill_voided", FillVoided, OrderFillVoidedSpec),
    ]
}

fn execution_positions() -> [(&'static str, PositionEvent); 4] {
    let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
    let mut fill = OrderFilledSpec::builder()
        .instrument_id(instrument.id())
        .position_id(PositionId::from("POSITION-1"))
        .last_qty(Quantity::from("2.750"))
        .last_px(Price::from("16384.00"))
        .currency(Currency::USDT())
        .commission(Money::from("0.73 USDT"))
        .ts_event((TS_BASE + 150).into())
        .ts_init((TS_BASE + 151).into())
        .build();
    let mut position = Position::new(&instrument, fill.clone());
    let opened = PositionOpened::create(&position, &fill, UUID4::new(), (TS_BASE + 152).into());
    fill.event_id = UUID4::new();
    fill.trade_id = TradeId::from("TRADE-2");
    fill.last_qty = Quantity::from("1.250");
    fill.last_px = Price::from("16384.00");
    fill.ts_event = (TS_BASE + 153).into();
    position.apply(&fill);
    let changed = PositionChanged::create(&position, &fill, UUID4::new(), (TS_BASE + 154).into());
    fill.event_id = UUID4::new();
    fill.client_order_id = ClientOrderId::from("ORDER-CLOSE-1");
    fill.venue_order_id = VenueOrderId::from("VENUE-CLOSE-1");
    fill.trade_id = TradeId::from("TRADE-3");
    fill.order_side = OrderSide::Sell;
    fill.last_qty = position.quantity;
    fill.last_px = Price::from("16386.00");
    fill.ts_event = (TS_BASE + 155).into();
    position.apply(&fill);
    let closed = PositionClosed::create(&position, &fill, UUID4::new(), (TS_BASE + 156).into());

    let adjusted = PositionAdjusted::new(
        position.trader_id,
        position.strategy_id,
        position.instrument_id,
        position.id,
        position.account_id,
        PositionAdjustmentType::Funding,
        None,
        Some(Money::from("1.25 USDT")),
        Some(ustr::Ustr::from("funding payment")),
        UUID4::new(),
        (TS_BASE + 157).into(),
        (TS_BASE + 158).into(),
    );
    [
        ("position_opened", PositionEvent::PositionOpened(opened)),
        ("position_changed", PositionEvent::PositionChanged(changed)),
        ("position_closed", PositionEvent::PositionClosed(closed)),
        (
            "position_adjusted",
            PositionEvent::PositionAdjusted(adjusted),
        ),
    ]
}

fn execution_reports() -> (FillReport, PositionStatusReport) {
    let fill = FillReport::new(
        AccountId::from("SIM-1"),
        InstrumentId::from("BTCUSDT.BINANCE"),
        VenueOrderId::from("VENUE-2"),
        TradeId::from("TRADE-1"),
        OrderSide::Buy,
        Quantity::from("2.750"),
        Price::from("20001.125"),
        Money::from("0.73 USD"),
        LiquiditySide::Taker,
        Some(ClientOrderId::from("ORDER-1")),
        Some(PositionId::from("POSITION-1")),
        (TS_BASE + 159).into(),
        (TS_BASE + 160).into(),
        Some(UUID4::new()),
    );

    let position = PositionStatusReport::new(
        fill.account_id,
        fill.instrument_id,
        PositionSide::Long,
        Quantity::from("3.750"),
        (TS_BASE + 161).into(),
        (TS_BASE + 162).into(),
        Some(UUID4::new()),
        fill.venue_position_id,
        Some(rust_decimal_macros::dec!(20000.123456789)),
    );
    (fill, position)
}

fn execution_concrete(name: &str, command: &TradingCommand, config: &Run<'_>) {
    let case = format!("{name}_concrete");
    macro_rules! concrete {
        ($value:expr, $ty:ty) => {{
            assert_eq!(
                serialize_payload(config.encoding, BusPayloadType::TradingCommand, command)
                    .unwrap(),
                serialize_payload(config.encoding, BusPayloadType::TradingCommand, $value).unwrap(),
            );
            run(
                &case,
                BusPayloadType::TradingCommand,
                $value,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<$ty>,
                config,
            );
        }};
    }

    match command {
        TradingCommand::SubmitOrder(value) => {
            concrete!(value, SubmitOrder);
        }
        TradingCommand::CancelOrder(value) => {
            concrete!(value, CancelOrder);
        }
        TradingCommand::ModifyOrder(value) => {
            concrete!(value, ModifyOrder);
        }
        TradingCommand::SubmitOrderList(value) => concrete!(value, SubmitOrderList),
        TradingCommand::ModifyOrders(value) => concrete!(value, BatchModifyOrders),
        TradingCommand::CancelOrders(value) => concrete!(value, BatchCancelOrders),
        TradingCommand::CancelAllOrders(value) => concrete!(value, CancelAllOrders),
        TradingCommand::QueryOrder(value) => concrete!(value, QueryOrder),
        TradingCommand::QueryAccount(value) => concrete!(value, QueryAccount),
    }
}

fn data_responses() -> Vec<(String, DataResponse)> {
    let names = [
        "custom_scalar",
        "custom_batch",
        "custom_empty",
        "custom_bytes",
        "custom_buffer",
        "custom_text",
        "instrument",
        "instruments",
        "book",
        "book_deltas",
        "book_depth",
        "quotes",
        "trades",
        "funding_rates",
        "option_chain_reference_price",
        "bars",
    ];
    let mut cases = Vec::new();

    for mode in [0, 1, 2] {
        for count in [1, 32, 256] {
            let values = responses(mode);
            assert_eq!(values.len(), names.len());

            for (name, mut value) in names.into_iter().zip(values) {
                if count == 1 {
                    cases.extend(instrument_responses(&value, mode));
                }

                if !resize_response(&mut value, count) {
                    continue;
                }

                cases.push((format!("response_{name}_mode{mode}_{count}"), value));
            }
        }
    }

    cases
}

fn instrument_responses(value: &DataResponse, mode: u8) -> Vec<(String, DataResponse)> {
    let DataResponse::Instrument(response) = value else {
        return Vec::new();
    };

    let mut cases = Vec::new();

    for (name, instrument) in instrument_cases() {
        let mut response = response.clone();
        response.data = instrument;
        cases.push((
            format!("response_instrument_{name}_mode{mode}"),
            DataResponse::Instrument(response),
        ));
    }

    cases
}

fn instrument_cases() -> [(&'static str, InstrumentAny); 17] {
    [
        ("betting", InstrumentAny::Betting(betting())),
        (
            "binary_option",
            InstrumentAny::BinaryOption(binary_option()),
        ),
        ("cfd", InstrumentAny::Cfd(cfd_gold())),
        ("commodity", InstrumentAny::Commodity(commodity_gold())),
        (
            "crypto_future",
            InstrumentAny::CryptoFuture(crypto_future_btcusdt(
                2,
                6,
                Price::from("0.01"),
                Quantity::from("0.000001"),
            )),
        ),
        (
            "crypto_futures_spread",
            InstrumentAny::CryptoFuturesSpread(crypto_futures_spread_btc_deribit()),
        ),
        (
            "crypto_option",
            InstrumentAny::CryptoOption(crypto_option_btc_deribit(
                3,
                1,
                Price::from("0.001"),
                Quantity::from("0.1"),
            )),
        ),
        (
            "crypto_option_spread",
            InstrumentAny::CryptoOptionSpread(crypto_option_spread_btc_deribit()),
        ),
        (
            "crypto_perpetual",
            InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()),
        ),
        ("equity", InstrumentAny::Equity(equity_aapl())),
        (
            "futures_contract",
            InstrumentAny::FuturesContract(futures_contract_es(None, None)),
        ),
        (
            "futures_spread",
            InstrumentAny::FuturesSpread(futures_spread_es()),
        ),
        (
            "index",
            InstrumentAny::IndexInstrument(index_instrument_spx()),
        ),
        (
            "option_contract",
            InstrumentAny::OptionContract(option_contract_appl()),
        ),
        (
            "option_spread",
            InstrumentAny::OptionSpread(option_spread()),
        ),
        (
            "perpetual_contract",
            InstrumentAny::PerpetualContract(perpetual_contract_eurusd()),
        ),
        (
            "tokenized_asset",
            InstrumentAny::TokenizedAsset(tokenized_asset_aaplx()),
        ),
    ]
}

fn resize_response(value: &mut DataResponse, count: usize) -> bool {
    macro_rules! resize {
        ($value:expr) => {
            if let Some(first) = $value.data.first().cloned() {
                let mut rows = $value.data.to_vec();
                rows.resize(count, first);
                $value.data = rows.into();
            }
        };
    }

    match value {
        DataResponse::Data(response) => {
            if let Some(values) = response.data.downcast_ref::<Vec<CustomData>>() {
                let mut values = values.clone();
                if let Some(first) = values.first().cloned() {
                    values.resize(count, first);
                }

                response.data = std::sync::Arc::new(values);
            } else if let Some(values) = response.data.downcast_ref::<Vec<u8>>() {
                response.data = std::sync::Arc::new(values.repeat(count));
            } else if let Some(values) = response.data.downcast_ref::<Bytes>() {
                response.data = std::sync::Arc::new(Bytes::from(values.repeat(count)));
            } else if let Some(value) = response.data.downcast_ref::<String>() {
                response.data = std::sync::Arc::new(value.repeat(count));
            }
        }
        DataResponse::Instruments(value) => {
            if let Some(first) = value.data.first().cloned() {
                value.data.resize(count, first);
            }
        }
        DataResponse::BookDeltas(value) => resize!(value),
        DataResponse::BookDepth(value) => resize!(value),
        DataResponse::Quotes(value) => resize!(value),
        DataResponse::Trades(value) => resize!(value),
        DataResponse::FundingRates(value) => resize!(value),
        DataResponse::Bars(value) => resize!(value),
        _ => {
            if count != 1 {
                return false;
            }
        }
    }

    true
}

fn data_requests() -> Vec<(String, RequestCommand)> {
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let client_id = ClientId::from("CLIENT-1");
    let bar_type: BarType = "BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL".parse().unwrap();
    let series_id = "DERIBIT:BTC:BTC:1800000000000000000".parse().unwrap();
    let data_type = stub_custom_data(77, -88, Some(params()), None).data_type;
    let mut cases = Vec::new();

    for populated in [false, true] {
        let start = populated.then(|| jiff::Timestamp::new(-91, -123_456_789).unwrap());
        let end = populated.then(|| jiff::Timestamp::new(92, 987_654_321).unwrap());
        let limit = populated.then(|| std::num::NonZeroUsize::new(256).unwrap());
        let depth = populated.then(|| std::num::NonZeroUsize::new(10).unwrap());
        let parameters = populated.then(params);
        let ts_init = UnixNanos::from(TS_BASE + 219);
        macro_rules! ranged {
            ($variant:ident, $ty:ident, $identity:expr) => {
                (
                    stringify!($variant),
                    RequestCommand::$variant($ty::new(
                        $identity,
                        start,
                        end,
                        limit,
                        Some(client_id),
                        UUID4::new(),
                        ts_init,
                        parameters.clone(),
                    )),
                )
            };
        }

        for (name, value) in [
            (
                "Data",
                RequestCommand::Data(RequestCustomData::new(
                    client_id,
                    data_type.clone(),
                    start,
                    end,
                    limit,
                    UUID4::new(),
                    ts_init,
                    parameters.clone(),
                )),
            ),
            (
                "Instrument",
                RequestCommand::Instrument(RequestInstrument::new(
                    instrument_id,
                    start,
                    end,
                    Some(client_id),
                    UUID4::new(),
                    ts_init,
                    parameters.clone(),
                )),
            ),
            (
                "Instruments",
                RequestCommand::Instruments(RequestInstruments::new(
                    start,
                    end,
                    Some(client_id),
                    Some("BINANCE".into()),
                    UUID4::new(),
                    ts_init,
                    parameters.clone(),
                )),
            ),
            (
                "BookSnapshot",
                RequestCommand::BookSnapshot(RequestBookSnapshot::new(
                    instrument_id,
                    depth,
                    Some(client_id),
                    UUID4::new(),
                    ts_init,
                    parameters.clone(),
                )),
            ),
            ranged!(BookDeltas, RequestBookDeltas, instrument_id),
            (
                "BookDepth",
                RequestCommand::BookDepth(RequestBookDepth::new(
                    instrument_id,
                    start,
                    end,
                    limit,
                    depth,
                    Some(client_id),
                    UUID4::new(),
                    ts_init,
                    parameters.clone(),
                )),
            ),
            ranged!(Quotes, RequestQuotes, instrument_id),
            ranged!(Trades, RequestTrades, instrument_id),
            ranged!(FundingRates, RequestFundingRates, instrument_id),
            (
                "OptionChainReferencePrice",
                RequestCommand::OptionChainReferencePrice(RequestOptionChainReferencePrice::new(
                    series_id,
                    instrument_id,
                    Some(client_id),
                    UUID4::new(),
                    ts_init,
                    parameters.clone(),
                )),
            ),
            ranged!(Bars, RequestBars, bar_type),
            (
                "Join",
                RequestCommand::Join(RequestJoin::new(
                    vec![UUID4::new(), UUID4::new()],
                    start,
                    end,
                    UUID4::new(),
                    ts_init,
                    parameters.clone(),
                    Some(UUID4::new()),
                )),
            ),
        ] {
            cases.push((format!("request_{name}_populated{populated}"), value));
        }
    }

    cases
}

fn data_subscriptions() -> Vec<(String, SubscribeCommand)> {
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let bar_type: BarType = "BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL".parse().unwrap();
    let series_id = "DERIBIT:BTC:BTC:1800000000000000000".parse().unwrap();
    let data_type = stub_custom_data(77, -88, None, None).data_type;
    let mut cases = Vec::new();

    for populated in [false, true] {
        macro_rules! subscription {
            ($variant:ident, $ty:ident, {$($field:ident: $value:expr),* $(,)?}) => {
                SubscribeCommand::$variant($ty {
                    $($field: $value,)*
                    client_id: Some(ClientId::from("CLIENT-1")),
                    venue: Some("BINANCE".into()),
                    command_id: UUID4::new(),
                    ts_init: (TS_BASE + 309).into(),
                    correlation_id: populated.then(UUID4::new),
                    params: populated.then(params),
                })
            };
        }

        for value in [
            subscription!(Data, SubscribeCustomData, {data_type: data_type.clone()}),
            subscription!(Instrument, SubscribeInstrument, {instrument_ids: vec![instrument_id]}),
            SubscribeCommand::Instruments(SubscribeInstruments {
                client_id: Some(ClientId::from("CLIENT-1")),
                venue: "BINANCE".into(),
                command_id: UUID4::new(),
                ts_init: (TS_BASE + 310).into(),
                correlation_id: populated.then(UUID4::new),
                params: populated.then(params),
            }),
            subscription!(BookDeltas, SubscribeBookDeltas, {instrument_ids: vec![instrument_id],
                book_type: BookType::L3_MBO,
                depth: std::num::NonZeroUsize::new(20), managed: true}),
            subscription!(BookDepth, SubscribeBookDepth, {instrument_ids: vec![instrument_id],
                book_type: BookType::L2_MBP,
                depth: std::num::NonZeroUsize::new(10), managed: false}),
            subscription!(BookSnapshots, SubscribeBookSnapshots, {instrument_ids: vec![instrument_id],
                book_type: BookType::L2_MBP,
                depth: std::num::NonZeroUsize::new(5), interval_ms: std::num::NonZeroUsize::new(250).unwrap()}),
            subscription!(Quotes, SubscribeQuotes, {instrument_ids: vec![instrument_id]}),
            subscription!(Trades, SubscribeTrades, {instrument_ids: vec![instrument_id]}),
            subscription!(Bars, SubscribeBars, {bar_types: vec![bar_type]}),
            subscription!(MarkPrices, SubscribeMarkPrices, {instrument_ids: vec![instrument_id]}),
            subscription!(IndexPrices, SubscribeIndexPrices, {instrument_ids: vec![instrument_id]}),
            subscription!(FundingRates, SubscribeFundingRates, {instrument_ids: vec![instrument_id]}),
            subscription!(InstrumentStatus, SubscribeInstrumentStatus, {instrument_ids: vec![instrument_id]}),
            subscription!(InstrumentClose, SubscribeInstrumentClose, {instrument_ids: vec![instrument_id]}),
            subscription!(OptionGreeks, SubscribeOptionGreeks, {instrument_ids: vec![instrument_id]}),
        ] {
            let name = match &value {
                SubscribeCommand::Data(_) => "data",
                SubscribeCommand::Instrument(_) => "instrument",
                SubscribeCommand::Instruments(_) => "instruments",
                SubscribeCommand::BookDeltas(_) => "book_deltas",
                SubscribeCommand::BookDepth(_) => "book_depth",
                SubscribeCommand::BookSnapshots(_) => "book_snapshots",
                SubscribeCommand::Quotes(_) => "quotes",
                SubscribeCommand::Trades(_) => "trades",
                SubscribeCommand::Bars(_) => "bars",
                SubscribeCommand::MarkPrices(_) => "mark_prices",
                SubscribeCommand::IndexPrices(_) => "index_prices",
                SubscribeCommand::FundingRates(_) => "funding_rates",
                SubscribeCommand::InstrumentStatus(_) => "instrument_status",
                SubscribeCommand::InstrumentClose(_) => "instrument_close",
                SubscribeCommand::OptionGreeks(_) => "option_greeks",
                SubscribeCommand::OptionChain(_) => unreachable!(),
            };

            cases.push((format!("{name}_populated{populated}"), value));
        }

        let batch = subscription!(Bars, SubscribeBars, {bar_types: vec![
            bar_type,
            BarType::from("BTCUSDT.BINANCE-5-MINUTE-LAST-EXTERNAL"),
            BarType::from("ETHUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
        ]});
        cases.push((format!("bars_batch_populated{populated}"), batch));

        for (name, strike_range) in [
            (
                "fixed",
                StrikeRange::Fixed(vec![Price::from("20000.125"), Price::from("21000.750")]),
            ),
            (
                "relative",
                StrikeRange::AtmRelative {
                    strikes_above: 7,
                    strikes_below: 3,
                },
            ),
            ("percent", StrikeRange::AtmPercent { pct: 12.5 }),
            (
                "delta",
                StrikeRange::Delta {
                    target: 0.35,
                    tolerance: 0.07,
                },
            ),
        ] {
            let value = subscription!(OptionChain, SubscribeOptionChain, {series_id: series_id,
                strike_range: strike_range, snapshot_interval_ms: populated.then_some(250)});
            cases.push((format!("option_chain_{name}_populated{populated}"), value));
        }
    }

    cases
}

fn market_data(config: &Run<'_>) {
    macro_rules! market {
        ($name:literal, $kind:ident, $value:expr, $ty:ty, $publish:ident, $subscribe:ident) => {{
            let value = $value;
            run(
                $name,
                BusPayloadType::$kind,
                &value,
                msgbus::$publish,
                |seen| {
                    msgbus::$subscribe(
                        TOPIC.into(),
                        TypedHandler::from(move |value: &$ty| {
                            black_box(value);
                            seen.set(seen.get() + 1);
                        }),
                        None,
                    );
                },
                config,
            );
        }};
    }
    market!(
        "market_deltas",
        OrderBookDeltas,
        stub_deltas(),
        OrderBookDeltas,
        publish_deltas,
        subscribe_book_deltas
    );
    market!(
        "market_depth",
        OrderBookDepth,
        stub_depth10(),
        OrderBookDepth,
        publish_depth,
        subscribe_book_depth
    );
    market!(
        "market_quote",
        QuoteTick,
        quote_ethusdt_binance(),
        QuoteTick,
        publish_quote,
        subscribe_quotes
    );
    market!(
        "market_trade",
        TradeTick,
        stub_trade_ethusdt_buy(),
        TradeTick,
        publish_trade,
        subscribe_trades
    );
    market!(
        "market_bar",
        Bar,
        stub_bar(),
        Bar,
        publish_bar,
        subscribe_bars
    );
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let mark = MarkPriceUpdate {
        instrument_id,
        value: Price::from("20001.125"),
        ts_event: (TS_BASE + 11).into(),
        ts_init: (TS_BASE + 12).into(),
    };

    market!(
        "market_mark",
        MarkPriceUpdate,
        mark,
        MarkPriceUpdate,
        publish_mark_price,
        subscribe_mark_prices
    );

    let index = IndexPriceUpdate {
        instrument_id,
        value: Price::from("19999.750"),
        ts_event: (TS_BASE + 13).into(),
        ts_init: (TS_BASE + 14).into(),
    };

    market!(
        "market_index",
        IndexPriceUpdate,
        index,
        IndexPriceUpdate,
        publish_index_price,
        subscribe_index_prices
    );

    let funding = FundingRateUpdate {
        instrument_id,
        rate: rust_decimal_macros::dec!(-0.0123456789),
        interval: Some(480),
        next_funding_ns: Some((TS_BASE + 1_000).into()),
        ts_event: (TS_BASE + 15).into(),
        ts_init: (TS_BASE + 16).into(),
    };

    market!(
        "market_funding",
        FundingRateUpdate,
        funding,
        FundingRateUpdate,
        publish_funding_rate,
        subscribe_funding_rates
    );

    let greeks = OptionGreeks {
        instrument_id,
        convention: Default::default(),
        greeks: OptionGreekValues {
            delta: 0.41,
            gamma: 0.02,
            theta: -0.03,
            vega: 0.14,
            rho: 0.05,
        },
        mark_iv: Some(0.51),
        bid_iv: Some(0.49),
        ask_iv: Some(0.53),
        underlying_price: Some(20_000.5),
        open_interest: Some(1234.75),
        ts_event: (TS_BASE + 17).into(),
        ts_init: (TS_BASE + 18).into(),
    };

    market!(
        "market_greeks",
        OptionGreeks,
        greeks,
        OptionGreeks,
        publish_option_greeks,
        subscribe_option_greeks
    );

    if matches!(
        config.profile,
        Some("codec_latency" | "decode_latency" | "encode" | "decode")
    ) {
        let values = [
            (
                "custom",
                Data::Custom(stub_custom_data(77, -88, Some(params()), None)),
            ),
            ("delta", Data::BookDelta(stub_delta())),
            ("quote", Data::Quote(quote_ethusdt_binance())),
            ("trade", Data::Trade(stub_trade_ethusdt_buy())),
            ("bar", Data::Bar(stub_bar())),
            ("mark", Data::MarkPrice(mark)),
            ("index", Data::IndexPrice(index)),
            ("funding", Data::FundingRate(funding)),
            ("greeks", Data::OptionGreeks(greeks)),
            ("status", Data::InstrumentStatus(stub_instrument_status())),
            ("close", Data::InstrumentClose(stub_instrument_close())),
            (
                "instrument",
                Data::Instrument(Box::new(InstrumentAny::CurrencyPair(
                    currency_pair_btcusdt(),
                ))),
            ),
        ];
        macro_rules! untagged {
            ($variant:ident, $kind:ident, $ty:ty, $value:expr) => {{
                let value = $value;
                let bytes = serialize_payload(
                    config.encoding,
                    BusPayloadType::$kind,
                    &Data::$variant(Box::new(value.clone())),
                )
                .unwrap();
                let decoded: $ty =
                    deserialize_payload(BusPayloadType::$kind, config.encoding, &bytes)
                        .unwrap()
                        .unwrap();
                assert_eq!(
                    serde_json::to_value(decoded).unwrap(),
                    serde_json::to_value(value).unwrap()
                );
                let error =
                    deserialize_payload::<Data>(BusPayloadType::$kind, config.encoding, &bytes)
                        .unwrap_err();
                assert_eq!(
                    error.root_cause().to_string(),
                    "Missing 'type' field in Data"
                );
            }};
        }
        untagged!(BookDeltas, OrderBookDeltas, OrderBookDeltas, stub_deltas());
        untagged!(BookDepth, OrderBookDepth, OrderBookDepth, stub_depth10());

        for (name, value) in values {
            run(
                &format!("generic_data_{name}_single"),
                BusPayloadType::DataResponse,
                &value,
                |_, _| unreachable!(),
                |_| unreachable!(),
                config,
            );

            for count in [1, 32, 256] {
                let values = vec![value.clone(); count];
                run(
                    &format!("generic_data_{name}_{count}"),
                    BusPayloadType::DataResponse,
                    &values,
                    |_, _| unreachable!(),
                    |_| unreachable!(),
                    config,
                );
            }
        }
    }
}

struct Run<'a> {
    samples: usize,
    warmup: usize,
    trial: usize,
    format: &'a str,
    encoding: SerializationEncoding,
    profile: Option<&'a str>,
    selected_case: Option<&'a str>,
}

type Publish<T> = fn(MStr<Topic>, &T);
struct Case<'a, T> {
    name: &'a str,
    kind: BusPayloadType,
    value: &'a T,
    publish: Publish<T>,
    install: fn(Rc<Cell<usize>>),
}

fn run<T: Serialize + DeserializeOwned + Any>(
    name: &str,
    kind: BusPayloadType,
    value: &T,
    publish: Publish<T>,
    install: fn(Rc<Cell<usize>>),
    config: &Run<'_>,
) {
    if config.selected_case.is_some_and(|value| value != name) {
        return;
    }

    if config.profile == Some("processor_latency") && !kind.is_typed_message() {
        return;
    }

    let payload = serialize_payload(config.encoding, kind, value).unwrap();
    let decoded: T = deserialize_payload(kind, config.encoding, &payload)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(value).unwrap()
    );

    wire_fixture(name, kind, value, &payload, config);
    for _ in 0..config.warmup {
        black_box(serialize_payload(config.encoding, kind, black_box(value)).unwrap());
        black_box(
            deserialize_payload::<T>(kind, config.encoding, black_box(&payload))
                .unwrap()
                .unwrap(),
        );
    }

    if config.profile == Some("processor_latency") {
        processor(name, kind, value, payload, install, config);
        return;
    }

    measure_codec(name, kind, value, &payload, config);
    if config.profile.is_some() {
        return;
    }

    let case = Case {
        name,
        kind,
        value,
        publish,
        install,
    };

    stages(&case, payload.len(), config, false);
    stages(&case, payload.len(), config, true);
    for (label, burst, retry) in [
        ("steady", 1, true),
        ("burst", 256, true),
        ("overload", 256, false),
    ] {
        delivery(&case, config, (label, burst, retry));
    }
}

fn measure_codec<T: Serialize + DeserializeOwned + Any>(
    name: &str,
    kind: BusPayloadType,
    value: &T,
    payload: &Bytes,
    config: &Run<'_>,
) {
    if config.profile == Some("decode_latency") {
        let mut times = Vec::with_capacity(config.samples);
        for _ in 0..config.samples {
            let started = Instant::now();
            let decoded = decode::<T>(config.encoding, kind, black_box(payload));
            times.push(started.elapsed().as_nanos() as u64);
            black_box(decoded);
        }

        emit(name, "decode", &times, payload.len(), config, &json!({}));
        return;
    }

    if let Some(operation) = config.profile.filter(|value| *value != "codec_latency") {
        profile_boundary();

        for _ in 0..config.samples {
            match operation {
                "encode" => {
                    black_box(encode(config.encoding, kind, black_box(value)));
                }
                "decode" => {
                    black_box(decode::<T>(config.encoding, kind, black_box(payload)));
                }
                _ => panic!("profile must be encode or decode"),
            }
        }

        println!(
            "{}",
            json!({"case": name, "format": config.format, "operation": operation, "profile_calls": config.samples, "bytes": payload.len()})
        );
        return;
    }

    let mut encoded = Vec::with_capacity(config.samples);
    let mut decoded_times = Vec::with_capacity(config.samples);
    for _ in 0..config.samples {
        let started = Instant::now();
        let bytes = encode(config.encoding, kind, black_box(value));
        encoded.push(started.elapsed().as_nanos() as u64);
        black_box(bytes);
        let started = Instant::now();
        let decoded = decode::<T>(config.encoding, kind, black_box(payload));
        decoded_times.push(started.elapsed().as_nanos() as u64);
        black_box(decoded);
    }

    emit(name, "encode", &encoded, payload.len(), config, &json!({}));
    emit(
        name,
        "decode",
        &decoded_times,
        payload.len(),
        config,
        &json!({}),
    );
}

fn wire_fixture<T: Serialize + DeserializeOwned + Any>(
    name: &str,
    kind: BusPayloadType,
    value: &T,
    payload: &Bytes,
    config: &Run<'_>,
) {
    let filename = format!("{name}.{}.json", config.format);

    if let Ok(directory) = std::env::var("NAUTILUS_BENCH_FIXTURE_DIR") {
        let directory = Path::new(&directory);
        fs::create_dir_all(directory).unwrap();
        let message = BusMessage::with_str_topic(TOPIC, kind, payload.clone(), config.encoding);
        let decoded: T = deserialize_payload(kind, config.encoding, payload)
            .unwrap()
            .unwrap();
        let reencoded = serialize_payload(config.encoding, kind, &decoded).unwrap();
        let fixture = json!({"case": name, "message": message, "reencoded": reencoded,
            "expected": serde_json::to_value(value).unwrap(),
            "diagnostic": name.starts_with("generic_"),
            "native_type": std::any::type_name::<T>()});
        fs::write(
            directory.join(&filename),
            serde_json::to_vec(&fixture).unwrap(),
        )
        .unwrap();
    }

    if let Ok(directory) = std::env::var("NAUTILUS_BENCH_REPLAY_DIR") {
        let bytes = fs::read(Path::new(&directory).join(&filename)).unwrap();
        let fixture: Value = serde_json::from_slice(&bytes).unwrap();
        let message: BusMessage = serde_json::from_value(fixture["message"].clone()).unwrap();
        assert_eq!(message.encoding, config.encoding);
        assert_eq!(message.payload_type, kind);
        let decoded: T = deserialize_payload(kind, config.encoding, &message.payload)
            .unwrap()
            .unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), fixture["expected"]);
        assert_eq!(
            serialize_payload(config.encoding, kind, &decoded).unwrap(),
            serde_json::from_value::<Bytes>(fixture["reencoded"].clone()).unwrap()
        );
    }
}

fn processor<T: Serialize + Any>(
    name: &str,
    kind: BusPayloadType,
    value: &T,
    payload: Bytes,
    install: fn(Rc<Cell<usize>>),
    config: &Run<'_>,
) {
    reset_bus();
    let mut bus = MessageBus::default();
    bus.add_streaming_type(kind);
    set_message_bus(Rc::new(RefCell::new(bus)));
    let seen = Rc::new(Cell::new(0));
    install(seen.clone());
    let message = BusMessage::with_str_topic(TOPIC, kind, payload, config.encoding);
    let mut expected = serde_json::to_value(value).unwrap();
    expected
        .as_object_mut()
        .unwrap()
        .insert("payload_type".into(), Value::from(kind.as_str()));
    msgbus::process_external_typed_message(&message, &mut |value, mapping| {
        assert!(value.downcast_ref::<T>().is_some());
        assert_eq!(mapping, &expected);
        Ok(())
    })
    .unwrap();

    let mut callback = |value: &dyn Any, mapping: &Value| {
        black_box(value);
        black_box(mapping);
        Ok(())
    };

    for _ in 0..config.warmup {
        msgbus::process_external_typed_message(black_box(&message), &mut callback).unwrap();
    }

    seen.set(0);
    let mut times = Vec::with_capacity(config.samples);
    for _ in 0..config.samples {
        let started = Instant::now();
        msgbus::process_external_typed_message(black_box(&message), &mut callback).unwrap();
        times.push(started.elapsed().as_nanos() as u64);
    }

    assert_eq!(seen.get(), config.samples);
    emit(
        name,
        "processor_and_local_publish",
        &times,
        message.payload.len(),
        config,
        &json!({}),
    );
}

#[inline(never)]
fn profile_boundary() {
    black_box(Box::new(0_u8));
}

#[inline(never)]
fn encode<T: Serialize + Any>(
    encoding: SerializationEncoding,
    kind: BusPayloadType,
    value: &T,
) -> Bytes {
    serialize_payload(encoding, kind, value).unwrap()
}

#[inline(never)]
fn decode<T: DeserializeOwned + Any>(
    encoding: SerializationEncoding,
    kind: BusPayloadType,
    payload: &[u8],
) -> T {
    deserialize_payload(kind, encoding, payload)
        .unwrap()
        .unwrap()
}

fn stages<T: Serialize + DeserializeOwned + Any>(
    case: &Case<'_, T>,
    bytes: usize,
    config: &Run<'_>,
    instrumented: bool,
) {
    let Case {
        name,
        kind,
        value,
        publish,
        install,
    } = *case;
    reset_bus();
    let seen = Rc::new(Cell::new(0));
    install(seen.clone());
    let mut samples = Vec::with_capacity(config.samples);
    let mut encoding_ns = 0_u128;
    let mut decoding_ns = 0_u128;
    let started_batch = Instant::now();

    for _ in 0..config.samples {
        let started = Instant::now();
        let encoded = serialize_payload(config.encoding, kind, black_box(value)).unwrap();
        let encoded_at = instrumented.then(Instant::now);
        let decoded: T = deserialize_payload(kind, config.encoding, &encoded)
            .unwrap()
            .unwrap();
        let decoded_at = instrumented.then(Instant::now);
        publish(TOPIC.into(), black_box(&decoded));
        samples.push(started.elapsed().as_nanos() as u64);
        if let (Some(encoded_at), Some(decoded_at)) = (encoded_at, decoded_at) {
            encoding_ns += encoded_at.duration_since(started).as_nanos();
            decoding_ns += decoded_at.duration_since(encoded_at).as_nanos();
        }
    }

    assert_eq!(seen.get(), config.samples);
    let total: u128 = samples.iter().map(|value| u128::from(*value)).sum();
    emit(
        name,
        if instrumented {
            "codec_and_local_publish_instrumented"
        } else {
            "codec_and_local_publish"
        },
        &samples,
        bytes,
        config,
        &json!({"encode_fraction": encoding_ns as f64 / total as f64, "decode_fraction": decoding_ns as f64 / total as f64, "messages_per_second": config.samples as f64 / started_batch.elapsed().as_secs_f64()}),
    );
}

#[derive(Debug)]
struct Queue {
    messages: RefCell<VecDeque<(Instant, BusMessage)>>,
    started: Cell<Instant>,
    dropped: Cell<usize>,
    peak: Cell<usize>,
}

impl MessageBusExternalEgress for Rc<Queue> {
    fn is_closed(&self) -> bool {
        false
    }

    fn close(&mut self) {}

    fn publish(&self, message: BusMessage) {
        let mut messages = self.messages.borrow_mut();
        if messages.len() == CAPACITY {
            self.dropped.set(self.dropped.get() + 1);
            return;
        }

        messages.push_back((self.started.get(), message));
        self.peak.set(self.peak.get().max(messages.len()));
    }
}

fn delivery<T: Serialize + Any>(
    case: &Case<'_, T>,
    config: &Run<'_>,
    workload: (&str, usize, bool),
) {
    let Case {
        name,
        kind,
        value,
        publish,
        install,
    } = *case;
    reset_bus();

    let queue = Rc::new(Queue {
        messages: RefCell::new(VecDeque::with_capacity(CAPACITY)),
        started: Cell::new(Instant::now()),
        dropped: Cell::new(0),
        peak: Cell::new(0),
    });

    let mut bus = MessageBus::default();
    bus.set_external_egress_config(
        Box::new(queue.clone()),
        &MessageBusConfig {
            encoding: config.encoding,
            external_streams: Some(vec!["benchmark".into()]),
            ..Default::default()
        },
    )
    .unwrap();

    bus.add_streaming_type(kind);
    set_message_bus(Rc::new(RefCell::new(bus)));
    let seen = Rc::new(Cell::new(0));
    install(seen.clone());
    let (label, burst, retry) = workload;
    let mut times = Vec::with_capacity(config.samples);
    let mut backpressure = 0;
    let started = Instant::now();

    for index in 0..config.samples {
        if retry && queue.messages.borrow().len() == CAPACITY {
            backpressure += 1;
            drain_one(&queue, &mut times);
        }

        queue.started.set(Instant::now());
        publish(TOPIC.into(), value);

        if (index + 1) % burst == 0 {
            while drain_one(&queue, &mut times) {}
        }
    }

    while drain_one(&queue, &mut times) {}
    // Each publication also invokes the local handler before external delivery
    assert_eq!(seen.get(), config.samples + times.len());
    assert_eq!(times.len() + queue.dropped.get(), config.samples);
    assert_eq!(queue.peak.get(), CAPACITY.min(config.samples).min(burst));

    let expected_dropped = if retry {
        0
    } else {
        (config.samples / burst) * burst.saturating_sub(CAPACITY)
            + (config.samples % burst).saturating_sub(CAPACITY)
    };

    assert_eq!(queue.dropped.get(), expected_dropped);
    emit(
        name,
        label,
        &times,
        0,
        config,
        &json!({"sent": config.samples, "delivered": times.len(), "dropped": queue.dropped.get(), "backpressure": backpressure, "queue_peak": queue.peak.get(), "messages_per_second": times.len() as f64 / started.elapsed().as_secs_f64()}),
    );
}

fn drain_one(queue: &Queue, times: &mut Vec<u64>) -> bool {
    let item = queue.messages.borrow_mut().pop_front();

    let Some((started, message)) = item else {
        return false;
    };

    msgbus::republish_external_message(&message).unwrap();
    times.push(started.elapsed().as_nanos() as u64);
    true
}

fn install_any<T: Any>(seen: Rc<Cell<usize>>) {
    msgbus::subscribe_any(
        TOPIC.into(),
        ShareableMessageHandler::from_typed(move |value: &T| {
            black_box(value);
            seen.set(seen.get() + 1);
        }),
        None,
    );
}

fn install_snapshot(seen: Rc<Cell<usize>>) {
    msgbus::subscribe_portfolio_snapshot(
        TOPIC.into(),
        TypedHandler::from(move |value: &PortfolioSnapshot| {
            black_box(value);
            seen.set(seen.get() + 1);
        }),
        None,
    );
}

fn install_order(seen: Rc<Cell<usize>>) {
    msgbus::subscribe_order_events(
        TOPIC.into(),
        TypedHandler::from(move |value: &OrderEventAny| {
            black_box(value);
            seen.set(seen.get() + 1);
        }),
        None,
    );
}

fn install_position(seen: Rc<Cell<usize>>) {
    msgbus::subscribe_position_events(
        TOPIC.into(),
        TypedHandler::from(move |value: &PositionEvent| {
            black_box(value);
            seen.set(seen.get() + 1);
        }),
        None,
    );
}

fn install_account(seen: Rc<Cell<usize>>) {
    msgbus::subscribe_account_state(
        TOPIC.into(),
        TypedHandler::from(move |value: &AccountState| {
            black_box(value);
            seen.set(seen.get() + 1);
        }),
        None,
    );
}

fn reset_bus() {
    msgbus::get_message_bus().borrow_mut().dispose();
    set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
}

fn setting(name: &str, default: usize) -> usize {
    let value = std::env::var(name).map_or(default, |value| value.parse().unwrap());
    assert!(value > 0);
    value
}

fn emit(name: &str, operation: &str, times: &[u64], bytes: usize, config: &Run<'_>, extra: &Value) {
    let mut sorted = times.to_vec();
    sorted.sort_unstable();
    assert!(!sorted.is_empty());

    let percentile = |numerator: usize, denominator: usize| {
        sorted[((sorted.len() - 1) * numerator) / denominator]
    };

    println!(
        "{}",
        json!({"case": name, "format": config.format, "trial": config.trial, "operation": operation, "samples": times.len(), "bytes": bytes, "mean_ns": times.iter().map(|value| *value as f64).sum::<f64>() / times.len() as f64, "p50_ns": percentile(50,100), "p95_ns": percentile(95,100), "p99_ns": percentile(99,100), "p999_ns": percentile(999,1000), "max_ns": sorted.last().unwrap(), "extra": extra})
    );
}
