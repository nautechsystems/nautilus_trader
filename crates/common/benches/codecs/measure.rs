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
    hint::black_box,
    rc::Rc,
    time::Instant,
};

use bytes::Bytes;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionAdjustmentType,
        PositionSide, TimeInForce,
    },
    events::{
        AccountState, OrderEventAny, PositionAdjusted, PositionChanged, PositionClosed,
        PositionEvent, PositionOpened, order::spec::*,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
        TradeId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::currency_pair_btcusdt},
    orders::OrderList,
    position::Position,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
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
        data::{DataResponse, RequestCommand, RequestJoin},
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
    let selected_case = std::env::var("NAUTILUS_BENCH_CASE").ok();
    let selected_format = std::env::var("NAUTILUS_BENCH_FORMAT").ok();
    let profile = std::env::var("NAUTILUS_BENCH_PROFILE").ok();
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

    let mut scalar_params = nautilus_core::Params::new();
    scalar_params.insert("post_only".into(), Value::Bool(true));
    scalar_params.insert("route".into(), Value::from("external"));

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
        value.instrument_id = value.data[0].instrument_id;
        value.data.resize(32, value.data[0]);
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
    let (fill_report, position_report) = execution_reports();

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

    let formats = [
        ("json", SerializationEncoding::Json),
        ("msgpack", SerializationEncoding::MsgPack),
    ];
    println!(
        "{}",
        json!({"environment": {"samples": samples, "trials": trials, "warmup": WARMUP, "queue_capacity": CAPACITY, "profile": profile}})
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
                trial,
                format,
                encoding,
                profile: profile.as_deref(),
                selected_case: selected_case.as_deref(),
            };

            run(
                "submit_order",
                BusPayloadType::TradingCommand,
                &submit,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<TradingCommand>,
                &config,
            );
            run(
                "cancel_order",
                BusPayloadType::TradingCommand,
                &cancel,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<TradingCommand>,
                &config,
            );
            run(
                "modify_order",
                BusPayloadType::TradingCommand,
                &modify,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<TradingCommand>,
                &config,
            );
            run(
                "query_account",
                BusPayloadType::TradingCommand,
                &query,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<TradingCommand>,
                &config,
            );
            run(
                "order_updated",
                BusPayloadType::OrderEvent,
                &updated,
                msgbus::publish_order_event,
                install_order,
                &config,
            );
            run(
                "order_filled",
                BusPayloadType::OrderEvent,
                &filled,
                msgbus::publish_order_event,
                install_order,
                &config,
            );
            run(
                "instrument_reply",
                BusPayloadType::DataResponse,
                &instrument,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<DataResponse>,
                &config,
            );
            run(
                "catalog_reply",
                BusPayloadType::DataResponse,
                &catalog,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<DataResponse>,
                &config,
            );
            run(
                "order_accepted",
                BusPayloadType::OrderEvent,
                &accepted,
                msgbus::publish_order_event,
                install_order,
                &config,
            );
            run(
                "order_status_report",
                BusPayloadType::OrderStatusReport,
                &report,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<OrderStatusReport>,
                &config,
            );
            run(
                "request_join",
                BusPayloadType::RequestCommand,
                &request,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<RequestCommand>,
                &config,
            );
            run(
                "historical_trades",
                BusPayloadType::DataResponse,
                &reply,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<DataResponse>,
                &config,
            );
            run(
                "custom_batch",
                BusPayloadType::DataResponse,
                &custom,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<DataResponse>,
                &config,
            );
            run(
                "account_state",
                BusPayloadType::AccountState,
                &account,
                msgbus::publish_account_state,
                install_account,
                &config,
            );

            run(
                "account_state_plain",
                BusPayloadType::AccountState,
                &account_plain,
                msgbus::publish_account_state,
                install_account,
                &config,
            );

            for (name, value) in &commands {
                run(
                    name,
                    BusPayloadType::TradingCommand,
                    value,
                    |topic, value| msgbus::publish_any(topic, value),
                    install_any::<TradingCommand>,
                    &config,
                );
            }

            for (name, value) in &events {
                run(
                    name,
                    BusPayloadType::OrderEvent,
                    value,
                    msgbus::publish_order_event,
                    install_order,
                    &config,
                );
            }

            for (name, value) in &positions {
                run(
                    name,
                    BusPayloadType::PositionEvent,
                    value,
                    msgbus::publish_position_event,
                    install_position,
                    &config,
                );
            }

            run(
                "fill_report",
                BusPayloadType::FillReport,
                &fill_report,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<FillReport>,
                &config,
            );
            run(
                "position_status_report",
                BusPayloadType::PositionStatusReport,
                &position_report,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<PositionStatusReport>,
                &config,
            );
            run(
                "generate_mass_status",
                BusPayloadType::GenerateExecutionMassStatus,
                &mass_request,
                |topic, value| msgbus::publish_any(topic, value),
                install_any::<GenerateExecutionMassStatus>,
                &config,
            );

            if config.profile == Some("decode_latency") {
                for (name, command) in [
                    ("submit_order", &submit),
                    ("cancel_order", &cancel),
                    ("modify_order", &modify),
                ] {
                    execution_concrete(name, command, &config);
                }

                for (name, command) in &commands {
                    if name.ends_with("_plain") {
                        execution_concrete(name, command, &config);
                    }
                }
            }
        }
    }

    reset_bus();
}

fn execution_commands(
    submit: &TradingCommand,
    cancel: &TradingCommand,
    modify: &TradingCommand,
) -> [(&'static str, TradingCommand); 10] {
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
            "submit_order_list",
            TradingCommand::SubmitOrderList(SubmitOrderList {
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
            }),
        ),
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
        _ => unreachable!(),
    }
}

struct Run<'a> {
    samples: usize,
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

    let payload = serialize_payload(config.encoding, kind, value).unwrap();
    let decoded: T = deserialize_payload(kind, config.encoding, &payload)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(value).unwrap()
    );

    for _ in 0..WARMUP {
        black_box(serialize_payload(config.encoding, kind, black_box(value)).unwrap());
        black_box(
            deserialize_payload::<T>(kind, config.encoding, black_box(&payload))
                .unwrap()
                .unwrap(),
        );
    }

    if config.profile == Some("decode_latency") {
        let mut times = Vec::with_capacity(config.samples);
        for _ in 0..config.samples {
            let started = Instant::now();
            let decoded = decode::<T>(config.encoding, kind, black_box(&payload));
            times.push(started.elapsed().as_nanos() as u64);
            black_box(decoded);
        }

        emit(name, "decode", &times, payload.len(), config, &json!({}));
        return;
    }

    if let Some(operation) = config.profile {
        for _ in 0..config.samples {
            match operation {
                "encode" => {
                    black_box(encode(config.encoding, kind, black_box(value)));
                }
                "decode" => {
                    black_box(decode::<T>(config.encoding, kind, black_box(&payload)));
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
        let decoded = decode::<T>(config.encoding, kind, black_box(&payload));
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
