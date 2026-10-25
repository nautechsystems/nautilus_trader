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
    cell::RefCell,
    io::{Read, Write},
    process::{Command, Stdio},
    rc::Rc,
    sync::Arc,
};

use bytes::Bytes;
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BatchView, BookOrder, CustomData, FundingRateUpdate, OrderBookDelta, stubs::*},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    events::{
        OrderEventAny,
        order::spec::{OrderAcceptedSpec, OrderInitializedSpec},
    },
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{InstrumentAny, stubs::currency_pair_btcusdt},
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use rstest::rstest;
use rust_decimal_macros::dec;
#[cfg(feature = "defi")]
use ustr::Ustr;

use super::*;
use crate::{
    messages::{
        data::*,
        execution::{SubmitOrder, TradingCommand},
    },
    msgbus::{
        self, BusMessage, MessageBus, MessageBusConfig, MessageBusExternalEgress, set_message_bus,
    },
};

#[rstest]
#[case(SerializationEncoding::Json)]
#[case(SerializationEncoding::MsgPack)]
fn responses_preserve_every_field(
    #[case] encoding: SerializationEncoding,
    #[values(0, 1, 2)] mode: u8,
    #[values(0, 1, 2)] view: u8,
) {
    for mut expected in responses(mode) {
        let mut expected_value = serde_json::to_value(&expected).unwrap();

        if view != 0 {
            macro_rules! narrow {
                ($variant:ident, $response:expr) => {{
                    let mut backing = $response.data.to_vec();
                    let len = if view == 1 { backing.len() } else { 0 };
                    let start = usize::from(!backing.is_empty());

                    if let Some(row) = backing.first().cloned() {
                        backing.insert(0, row.clone());
                        backing.push(row);
                    }

                    $response.data = BatchView::new(Arc::new(backing), start..start + len);
                    expected_value[stringify!($variant)]["data"] =
                        serde_json::to_value($response.data.as_ref()).unwrap();
                }};
            }

            match &mut expected {
                DataResponse::BookDeltas(r) => narrow!(BookDeltas, r),
                DataResponse::BookDepth(r) => narrow!(BookDepth, r),
                DataResponse::Quotes(r) => narrow!(Quotes, r),
                DataResponse::Trades(r) => narrow!(Trades, r),
                DataResponse::FundingRates(r) => narrow!(FundingRates, r),
                DataResponse::Bars(r) => narrow!(Bars, r),
                _ => {}
            }
        }

        let payload = serialize_payload(encoding, BusPayloadType::DataResponse, &expected).unwrap();
        let decoded: DataResponse =
            deserialize_payload(BusPayloadType::DataResponse, encoding, &payload)
                .unwrap()
                .unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), expected_value);

        let envelope = DataResponseEnvelope::new(expected, false, Some(UUID4::new()));
        let payload =
            serialize_payload(encoding, BusPayloadType::DataResponseEnvelope, &envelope).unwrap();
        let decoded: DataResponseEnvelope =
            deserialize_payload(BusPayloadType::DataResponseEnvelope, encoding, &payload)
                .unwrap()
                .unwrap();
        assert_eq!(
            serde_json::to_value(decoded.response().unwrap()).unwrap(),
            expected_value
        );
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&envelope).unwrap()
        );
    }
}

#[rstest]
#[case(SerializationEncoding::Json)]
#[case(SerializationEncoding::MsgPack)]
fn custom_response_rejects_non_custom_after_wire_decode(
    #[case] encoding: SerializationEncoding,
    #[values(false, true)] invalid_params: bool,
) {
    let response = responses(1).into_iter().next().unwrap();
    let mut value = serde_json::to_value(response).unwrap();
    value["Data"]["data"]["Scalar"]["value"] =
        serde_json::to_value(quote_ethusdt_binance()).unwrap();
    let invalid = serde_json::Value::Bool(true);
    if invalid_params {
        value["Data"]["params"] = invalid.clone();
    }

    let (actual, params_error) = match encoding {
        SerializationEncoding::Json => (
            serde_json::from_value::<DataResponse>(value).map_err(|e| e.to_string()),
            serde_json::from_value::<Params>(invalid)
                .unwrap_err()
                .to_string(),
        ),
        SerializationEncoding::MsgPack => (
            rmp_serde::from_slice::<DataResponse>(&rmp_serde::to_vec_named(&value).unwrap())
                .map_err(|e| e.to_string()),
            rmp_serde::from_slice::<Params>(&rmp_serde::to_vec_named(&invalid).unwrap())
                .unwrap_err()
                .to_string(),
        ),
        SerializationEncoding::Sbe => unreachable!(),
    };

    let expected = if invalid_params {
        params_error
    } else {
        "custom response contains a non-custom payload".to_string()
    };

    assert_eq!(actual.err().unwrap(), expected);
}

#[rstest]
#[case(SerializationEncoding::Json)]
#[case(SerializationEncoding::MsgPack)]
fn l1_book_response_starts_new_snapshot_after_roundtrip(#[case] encoding: SerializationEncoding) {
    let mut expected = responses(1)
        .into_iter()
        .find(|response| matches!(response, DataResponse::Book(_)))
        .unwrap();

    let DataResponse::Book(response) = &mut expected else {
        unreachable!()
    };

    response.data = OrderBook::new(response.instrument_id, BookType::L1_MBP);
    let flags = RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8;
    response.data.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("100.125"),
            Quantity::from("1.250"),
            41,
        ),
        flags,
        17,
        88.into(),
    );
    response.data.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("101.875"),
            Quantity::from("2.500"),
            42,
        ),
        flags,
        18,
        89.into(),
    );
    let payload = serialize_payload(encoding, BusPayloadType::DataResponse, &expected).unwrap();
    let mut decoded: DataResponse =
        deserialize_payload(BusPayloadType::DataResponse, encoding, &payload)
            .unwrap()
            .unwrap();

    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );

    for value in [&mut expected, &mut decoded] {
        let DataResponse::Book(response) = value else {
            unreachable!()
        };

        response.data.add(
            BookOrder::new(
                OrderSide::Buy,
                Price::from("90.375"),
                Quantity::from("3.750"),
                43,
            ),
            flags,
            19,
            90.into(),
        );
        response.data.add(
            BookOrder::new(
                OrderSide::Sell,
                Price::from("111.125"),
                Quantity::from("4.125"),
                44,
            ),
            flags,
            20,
            91.into(),
        );
    }

    let DataResponse::Book(response) = &decoded else {
        unreachable!()
    };

    assert_eq!(response.data.best_bid_price(), Some(Price::from("90.375")));
    assert_eq!(response.data.best_bid_size(), Some(Quantity::from("3.750")));
    assert_eq!(response.data.best_ask_price(), Some(Price::from("111.125")));
    assert_eq!(response.data.best_ask_size(), Some(Quantity::from("4.125")));
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[rstest]
#[case(SerializationEncoding::Json)]
#[case(SerializationEncoding::MsgPack)]
fn native_egress_processor_ingress_and_cross_process_causation(
    #[case] encoding: SerializationEncoding,
) {
    let recorded = install_bus(encoding);
    let correlation_id = UUID4::new();

    let command = RequestCommand::Join(RequestJoin {
        request_ids: vec![UUID4::new(), UUID4::new()],
        start: Some(jiff::Timestamp::new(-91, -123_456_789).unwrap()),
        end: Some(jiff::Timestamp::new(92, 987_654_321).unwrap()),
        request_id: UUID4::new(),
        ts_init: UnixNanos::from(101_u64),
        params: Some(params()),
        correlation_id: Some(correlation_id),
    });

    msgbus::publish_any("control.requests".into(), &command);
    let mut response = responses(1).remove(1);
    if let DataResponse::Data(value) = &mut response {
        value.correlation_id = *command.request_id();
    }

    msgbus::publish_any("control.responses".into(), &response);
    let mut init = OrderInitializedSpec::builder()
        .event_id(UUID4::new())
        .build();
    init.causation_id = Some(correlation_id);

    let submit = SubmitOrder {
        trader_id: init.trader_id,
        client_id: Some(ClientId::from("CLIENT-001")),
        strategy_id: init.strategy_id,
        instrument_id: init.instrument_id,
        client_order_id: init.client_order_id,
        order_init: init,
        exec_algorithm_id: None,
        position_id: None,
        params: Some(params()),
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(99_u64),
        correlation_id: Some(correlation_id),
        causation_id: Some(UUID4::new()),
    };

    let cause = submit.command_id;
    let submit = TradingCommand::SubmitOrder(submit);
    msgbus::publish_any("control.trading".into(), &submit);
    let mut event = OrderAcceptedSpec::builder()
        .event_id(UUID4::new())
        .ts_event(UnixNanos::from(22_u64))
        .ts_init(UnixNanos::from(33_u64))
        .build();
    event.causation_id = Some(cause);
    let event = OrderEventAny::Accepted(event);
    msgbus::publish_order_event("events.orders".into(), &event);
    let messages = recorded.borrow().clone();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].payload_type, BusPayloadType::RequestCommand);
    assert_eq!(messages[1].payload_type, BusPayloadType::DataResponse);
    assert_eq!(messages[2].payload_type, BusPayloadType::TradingCommand);
    assert_eq!(messages[3].payload_type, BusPayloadType::OrderEvent);

    for message in &messages {
        assert_eq!(message.encoding, encoding);
    }

    let observed = Rc::new(RefCell::new(Vec::new()));
    let saved = observed.clone();
    msgbus::subscribe_any(
        "control.requests".into(),
        msgbus::ShareableMessageHandler::from_typed(move |value: &RequestCommand| {
            saved
                .borrow_mut()
                .push(serde_json::to_value(value).unwrap());
        }),
        None,
    );

    msgbus::get_message_bus()
        .borrow_mut()
        .add_streaming_type(BusPayloadType::RequestCommand);
    let mut mapped = Vec::new();
    msgbus::process_external_typed_message(&messages[0], &mut |value, mapping| {
        assert_eq!(
            value.downcast_ref::<RequestCommand>().unwrap().request_id(),
            command.request_id()
        );
        mapped.push(mapping.clone());
        Ok(())
    })
    .unwrap();

    assert_eq!(
        *observed.borrow(),
        vec![serde_json::to_value(&command).unwrap()]
    );
    assert_eq!(recorded.borrow().len(), 4);
    assert_eq!(mapped.len(), 1);

    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("decoded.json");

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "msgbus::external::codec::native_tests::process_worker",
            "--ignored",
        ])
        .env("NAUTILUS_CODEC_TEST_OUTPUT", &output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&messages).unwrap())
        .unwrap();
    let child = child.wait_with_output().unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let decoded: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(
        decoded,
        vec![
            serde_json::to_value(&command).unwrap(),
            serde_json::to_value(&response).unwrap(),
            serde_json::to_value(&submit).unwrap(),
            serde_json::to_value(&event).unwrap()
        ]
    );
    msgbus::get_message_bus().borrow_mut().dispose();
    set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
}

#[rstest]
#[ignore = "subprocess target for the cross-process roundtrip test"]
fn process_worker() {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).unwrap();
    let messages: Vec<BusMessage> = serde_json::from_slice(&input).unwrap();
    ensure_stub_custom_data_registered();
    let values = Rc::new(RefCell::new(Vec::new()));
    let saved = values.clone();
    msgbus::subscribe_any(
        "control.requests".into(),
        msgbus::ShareableMessageHandler::from_typed(move |value: &RequestCommand| {
            saved
                .borrow_mut()
                .push(serde_json::to_value(value).unwrap());
        }),
        None,
    );

    let saved = values.clone();
    msgbus::subscribe_any(
        "control.responses".into(),
        msgbus::ShareableMessageHandler::from_typed(move |value: &DataResponse| {
            saved
                .borrow_mut()
                .push(serde_json::to_value(value).unwrap());
        }),
        None,
    );

    let saved = values.clone();
    msgbus::subscribe_any(
        "control.trading".into(),
        msgbus::ShareableMessageHandler::from_typed(move |value: &TradingCommand| {
            saved
                .borrow_mut()
                .push(serde_json::to_value(value).unwrap());
        }),
        None,
    );

    let saved = values.clone();
    msgbus::subscribe_order_events(
        "events.orders".into(),
        msgbus::TypedHandler::from(move |value: &OrderEventAny| {
            saved
                .borrow_mut()
                .push(serde_json::to_value(value).unwrap());
        }),
        None,
    );

    for message in messages {
        msgbus::get_message_bus()
            .borrow_mut()
            .add_streaming_type(message.payload_type);
        msgbus::republish_external_message(&message).unwrap();
    }

    std::fs::write(
        std::env::var_os("NAUTILUS_CODEC_TEST_OUTPUT").unwrap(),
        serde_json::to_vec(&*values.borrow()).unwrap(),
    )
    .unwrap();
}

#[cfg(feature = "defi")]
#[rstest]
#[case(SerializationEncoding::Json)]
#[case(SerializationEncoding::MsgPack)]
fn defi_block_external_ingress_preserves_chain(#[case] encoding: SerializationEncoding) {
    use nautilus_model::defi::{Block, Blockchain};

    let mut expected = Block::new(
        "hash".into(),
        "parent".into(),
        71,
        Ustr::from("miner"),
        301,
        209,
        47.into(),
        None,
    );
    expected.chain = Some(Blockchain::Arbitrum);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let saved = observed.clone();
    msgbus::subscribe_defi_blocks(
        "defi.blocks".into(),
        msgbus::TypedHandler::from(move |value: &Block| {
            saved
                .borrow_mut()
                .push((value.chain, serde_json::to_value(value).unwrap()));
        }),
        None,
    );

    msgbus::get_message_bus()
        .borrow_mut()
        .add_streaming_type(BusPayloadType::Block);
    let payload = serialize_payload(encoding, BusPayloadType::Block, &expected).unwrap();
    let message =
        BusMessage::with_str_topic("defi.blocks", BusPayloadType::Block, payload, encoding);
    msgbus::republish_external_message(&message).unwrap();
    assert_eq!(
        *observed.borrow(),
        vec![(
            Some(Blockchain::Arbitrum),
            serde_json::to_value(expected).unwrap()
        )]
    );
    msgbus::get_message_bus().borrow_mut().dispose();
    set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
}

#[derive(Debug)]
struct RecordingEgress(Rc<RefCell<Vec<BusMessage>>>);

impl MessageBusExternalEgress for RecordingEgress {
    fn is_closed(&self) -> bool {
        false
    }

    fn publish(&self, message: BusMessage) {
        self.0.borrow_mut().push(message);
    }

    fn close(&mut self) {}
}

fn install_bus(encoding: SerializationEncoding) -> Rc<RefCell<Vec<BusMessage>>> {
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let mut bus = MessageBus::default();

    let config = MessageBusConfig {
        encoding,
        encoding_market_data: cfg!(feature = "sbe").then_some(SerializationEncoding::Sbe),
        external_streams: Some(vec!["input".into()]),
        ..Default::default()
    };

    bus.set_external_egress_config(Box::new(RecordingEgress(recorded.clone())), &config)
        .unwrap();
    set_message_bus(Rc::new(RefCell::new(bus)));
    recorded
}

pub(super) fn params() -> Params {
    serde_json::from_str(include_str!("../../../../test_data/message_params.json")).unwrap()
}

pub(super) fn responses(mode: u8) -> Vec<DataResponse> {
    let populated = mode != 0;
    let empty = mode == 2;
    let parameters = populated.then(|| if empty { Params::empty() } else { params() });
    let mut book = OrderBook::new(InstrumentId::from("BTCUSDT.BINANCE"), BookType::L3_MBO);

    if !empty {
        let mut order = stub_book_order();
        order.side = Some(OrderSide::Sell);
        order.price = Price::from("100.125");
        order.size = Quantity::from("3.750");
        book.add(order, 0, 17, UnixNanos::from(88_u64));
    }

    book.sequence = 91;
    book.ts_last = UnixNanos::from(92_u64);
    book.update_count = 93;
    let custom = stub_custom_data(77, -88, Some(params()), Some("native-custom".into()));
    let mut values = Vec::new();
    for data in [
        std::sync::Arc::new(custom.clone()) as Payload,
        std::sync::Arc::new(vec![custom.clone()]) as Payload,
        std::sync::Arc::new(Vec::<CustomData>::new()) as Payload,
        std::sync::Arc::new(vec![0_u8, 37, 255]) as Payload,
        std::sync::Arc::new(Bytes::from_static(b"buffer")) as Payload,
        std::sync::Arc::new("text-payload".to_owned()) as Payload,
    ] {
        values.push(DataResponse::Data(CustomDataResponse {
            correlation_id: UUID4::new(),
            client_id: ClientId::from("CLIENT-001"),
            venue: populated.then_some(Venue::from("SIM")),
            data_type: custom.data_type.clone(),
            data,
            start: populated.then_some(UnixNanos::from(0_u64)),
            end: populated.then_some(UnixNanos::from(106_u64)),
            ts_init: UnixNanos::from(107_u64),
            params: parameters.clone(),
        }));
    }

    values.push(DataResponse::Instrument(Box::new(InstrumentResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        data: InstrumentAny::CurrencyPair(currency_pair_btcusdt()),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    })));

    values.push(DataResponse::Instruments(InstrumentsResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        venue: Venue::from("SIM"),
        data: (!empty)
            .then(|| InstrumentAny::CurrencyPair(currency_pair_btcusdt()))
            .into_iter()
            .collect(),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    }));

    values.push(DataResponse::Book(BookResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        data: book.clone(),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    }));

    values.push(DataResponse::BookDeltas(BookDeltasResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        data: ((!empty).then(stub_delta).into_iter().collect::<Vec<_>>()).into(),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    }));

    values.push(DataResponse::BookDepth(BookDepthResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        data: ((!empty).then(stub_depth10).into_iter().collect::<Vec<_>>()).into(),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    }));

    values.push(DataResponse::Quotes(QuotesResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        instrument_ids: vec![InstrumentId::from("BTCUSDT.BINANCE")],
        data: ((!empty)
            .then(quote_ethusdt_binance)
            .into_iter()
            .collect::<Vec<_>>())
        .into(),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    }));

    values.push(DataResponse::Trades(TradesResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        instrument_ids: vec![InstrumentId::from("BTCUSDT.BINANCE")],
        data: ((!empty)
            .then(stub_trade_ethusdt_buy)
            .into_iter()
            .collect::<Vec<_>>())
        .into(),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    }));

    values.push(DataResponse::FundingRates(FundingRatesResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        data: ((!empty)
            .then(|| FundingRateUpdate {
                instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
                rate: dec!(-0.0123456789),
                interval: Some(0),
                next_funding_ns: Some(UnixNanos::from(0_u64)),
                ts_event: UnixNanos::from(123_u64),
                ts_init: UnixNanos::from(456_u64),
            })
            .into_iter()
            .collect::<Vec<_>>())
        .into(),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(105_u64)),
        ts_init: UnixNanos::from(106_u64),
        params: parameters.clone(),
    }));

    values.push(DataResponse::OptionChainReferencePrice(
        OptionChainReferencePriceResponse {
            correlation_id: UUID4::new(),
            client_id: ClientId::from("CLIENT-001"),
            series_id: "DERIBIT:BTC:BTC:1800000000000000000".parse().unwrap(),
            price: populated.then_some(Price::from("13.125")),
            ts_init: UnixNanos::from(104_u64),
            params: parameters.clone(),
        },
    ));

    values.push(DataResponse::Bars(BarsResponse {
        correlation_id: UUID4::new(),
        client_id: ClientId::from("CLIENT-001"),
        bar_type: "BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL".parse().unwrap(),
        data: ((!empty).then(stub_bar).into_iter().collect::<Vec<_>>()).into(),
        ts_init: UnixNanos::from(104_u64),
        start: populated.then_some(UnixNanos::from(0_u64)),
        end: populated.then_some(UnixNanos::from(106_u64)),
        params: parameters,
    }));

    values
}

#[rstest]
#[case(0, 0)]
#[case(RecordFlag::F_MBP as u8, 1)]
#[case(RecordFlag::F_SNAPSHOT as u8, 2)]
fn test_l1_reply_retains_subsequent_batch_behavior(
    #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
    encoding: SerializationEncoding,
    #[case] flags: u8,
    #[case] state: u8,
    #[values(RecordFlag::F_MBP as u8, RecordFlag::F_SNAPSHOT as u8)] next_flags: u8,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let instrument_id = InstrumentId::from("EURUSD.SIM");
    let mut original = OrderBook::new(instrument_id, BookType::L1_MBP);
    original.add(
        BookOrder::new(OrderSide::Buy, Price::from("100"), Quantity::from("2"), 1),
        if side == OrderSide::Buy { flags } else { 0 },
        11,
        13.into(),
    );
    original.add(
        BookOrder::new(OrderSide::Sell, Price::from("101"), Quantity::from("3"), 2),
        if side == OrderSide::Sell { flags } else { 0 },
        17,
        19.into(),
    );
    let reply = DataResponse::Book(BookResponse::new(
        UUID4::new(),
        ClientId::from("SOURCE"),
        instrument_id,
        original.clone(),
        None,
        None,
        23.into(),
        None,
    ));

    let DataResponse::Book(mut decoded) = deserialize_payload::<DataResponse>(
        BusPayloadType::DataResponse,
        encoding,
        &serialize_payload(encoding, BusPayloadType::DataResponse, &reply).unwrap(),
    )
    .unwrap()
    .unwrap() else {
        panic!("Expected book response");
    };

    assert_eq!(decoded.data.bids_as_map(None), original.bids_as_map(None));
    assert_eq!(decoded.data.asks_as_map(None), original.asks_as_map(None));

    assert_eq!(decoded.data.batch_state_code(side), state);
    let other_side = side.opposite();
    assert_eq!(decoded.data.batch_state_code(other_side), 0);
    let retains_previous = flags == next_flags;

    let price = if side == OrderSide::Buy { "99" } else { "102" };

    let next = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(side, Price::from(price), Quantity::from("5"), 3),
        next_flags | RecordFlag::F_LAST as u8,
        29,
        31.into(),
        37.into(),
    );
    original.apply_delta(&next).unwrap();
    decoded.data.apply_delta(&next).unwrap();

    let expected_bid = if side == OrderSide::Buy && !retains_previous {
        "99"
    } else {
        "100"
    };

    let expected_ask = if side == OrderSide::Sell && !retains_previous {
        "102"
    } else {
        "101"
    };

    assert_eq!(original.best_bid_price(), Some(Price::from(expected_bid)));
    assert_eq!(original.best_ask_price(), Some(Price::from(expected_ask)));
    assert_eq!(decoded.data.bids_as_map(None), original.bids_as_map(None));
    assert_eq!(decoded.data.asks_as_map(None), original.asks_as_map(None));
    assert_eq!(decoded.data.batch_state_code(side), 0);
    assert_eq!(decoded.data.batch_state_code(other_side), 0);
    assert_eq!(decoded.data.sequence, original.sequence);
    assert_eq!(decoded.data.ts_last, original.ts_last);
    assert_eq!(decoded.data.update_count, original.update_count);
}

#[rstest]
fn test_book_response_preserves_full_depth(
    #[values(BookType::L2_MBP, BookType::L3_MBO)] book_type: BookType,
    #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
    encoding: SerializationEncoding,
    #[values(20_000)] order_count: usize,
) {
    let instrument_id = InstrumentId::from("EURUSD.SIM");
    let mut book = OrderBook::new(instrument_id, book_type);

    for index in 0..order_count {
        let side = if index % 2 == 0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let price = if side == OrderSide::Buy {
            index + 1
        } else {
            index + 100_000
        };

        book.add(
            BookOrder::new(
                side,
                Price::from(price.to_string().as_str()),
                Quantity::from((index % 97 + 1).to_string().as_str()),
                index as u64 + 11,
            ),
            0,
            index as u64 + 13,
            (index as u64 + 17).into(),
        );
    }

    let response = BookResponse::new(
        UUID4::new(),
        ClientId::from("SOURCE"),
        instrument_id,
        book,
        None,
        None,
        30_000.into(),
        None,
    );

    let DataResponse::Book(result) = deserialize_payload::<DataResponse>(
        BusPayloadType::DataResponse,
        encoding,
        &serialize_payload(
            encoding,
            BusPayloadType::DataResponse,
            &DataResponse::Book(response.clone()),
        )
        .unwrap(),
    )
    .unwrap()
    .unwrap() else {
        panic!("Expected book response");
    };

    let expected = response
        .data
        .to_deltas(response.data.ts_last, response.ts_init);
    let actual = result.data.to_deltas(result.data.ts_last, result.ts_init);
    assert_eq!(expected.deltas.len(), order_count + 1);
    assert_eq!(actual.deltas, expected.deltas);
    assert_eq!(actual.flags, expected.flags);
    assert_eq!(actual.sequence, expected.sequence);
    assert_eq!(actual.ts_event, expected.ts_event);
    assert_eq!(actual.ts_init, expected.ts_init);
    assert_eq!(result.data.book_type, book_type);
    assert_eq!(result.data.update_count, response.data.update_count);
}

#[rstest]
#[case(BookType::L1_MBP, 3, "Invalid L1 batch state code: 3")]
#[case(
    BookType::L2_MBP,
    1,
    "Cannot restore L1 batch state for book type L2_MBP"
)]
fn book_response_rejects_invalid_batch_state(
    #[case] book_type: BookType,
    #[case] code: u8,
    #[case] message: &str,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let instrument_id = InstrumentId::from("EURUSD.SIM");
    let book = OrderBook::new(instrument_id, book_type);
    let response = DataResponse::Book(BookResponse::new(
        UUID4::new(),
        ClientId::from("SOURCE"),
        instrument_id,
        book,
        None,
        None,
        23.into(),
        None,
    ));
    let mut value = serde_json::to_value(&response).unwrap();

    let name = match side {
        OrderSide::Buy => "bid_batch_state",
        OrderSide::Sell => "ask_batch_state",
    };

    value["Book"]["data"][name] = serde_json::json!(code);
    assert_eq!(
        serde_json::from_value::<DataResponse>(value)
            .unwrap_err()
            .to_string(),
        message
    );
}

#[rstest]
fn book_response_accepts_legacy_state_without_batch_fields() {
    let response = responses(1)
        .into_iter()
        .find(|value| matches!(value, DataResponse::Book(_)))
        .unwrap();
    let mut value = serde_json::to_value(&response).unwrap();
    let fields = value["Book"]["data"].as_object_mut().unwrap();
    assert_eq!(fields.remove("bid_batch_state"), Some(serde_json::json!(0)));
    assert_eq!(fields.remove("ask_batch_state"), Some(serde_json::json!(0)));
    let decoded: DataResponse = serde_json::from_value(value).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(response).unwrap()
    );
}

#[cfg(feature = "sbe")]
#[rstest]
fn mixed_sbe_market_data_and_msgpack_control_messages() {
    use nautilus_model::data::QuoteTick;
    use nautilus_serialization::sbe::FromSbe;

    let recorded = install_bus(SerializationEncoding::MsgPack);
    let quote = quote_ethusdt_binance();
    let response = responses(1).remove(1);
    msgbus::publish_quote("data.quotes".into(), &quote);
    msgbus::publish_any("control.responses".into(), &response);
    let messages = recorded.borrow();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].payload_type, BusPayloadType::QuoteTick);
    assert_eq!(messages[0].encoding, SerializationEncoding::Sbe);
    assert_eq!(QuoteTick::from_sbe(&messages[0].payload).unwrap(), quote);
    assert_eq!(messages[1].payload_type, BusPayloadType::DataResponse);
    assert_eq!(messages[1].encoding, SerializationEncoding::MsgPack);
    let decoded: DataResponse = deserialize_payload(
        BusPayloadType::DataResponse,
        messages[1].encoding,
        &messages[1].payload,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(response).unwrap()
    );
}

#[rstest]
#[case(SerializationEncoding::Json)]
#[case(SerializationEncoding::MsgPack)]
fn test_historical_envelope_round_trip(
    #[case] encoding: SerializationEncoding,
    #[values(false, true)] terminal: bool,
) {
    use crate::messages::data::DataResponseEnvelope;
    let response = responses(1).remove(0);
    let root = *response.correlation_id();
    let expected = if terminal {
        DataResponseEnvelope::terminal(root, Some("dataset unavailable".to_owned()))
    } else {
        DataResponseEnvelope::new(response, false, Some(UUID4::new()))
    };

    let payload =
        serialize_payload(encoding, BusPayloadType::DataResponseEnvelope, &expected).unwrap();
    let actual: DataResponseEnvelope =
        deserialize_payload(BusPayloadType::DataResponseEnvelope, encoding, &payload)
            .unwrap()
            .unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}
