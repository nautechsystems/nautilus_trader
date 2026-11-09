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

use std::fmt::Debug;

use nautilus_kalshi::{
    common::enums::*,
    http::{
        models::{BookSide, OrderStatus, OutcomeSide, SelfTradePrevention, TimeInForce},
        query::PositionsQuery,
    },
    websocket::{
        messages::{KalshiChannel, KalshiDataMessage, KalshiUpdateAction, KalshiWsMessage},
        parse::decode_frame,
    },
};
use rstest::rstest;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use strum::IntoEnumIterator;

#[rstest]
fn enums_match_every_documented_wire_value() {
    let fixture = fixture();
    assert_values(&fixture, "ExitTriggerKind", ExitTriggerKind::iter());
    assert_values(&fixture, "CandlestickInterval", CandlestickInterval::iter());
    assert_values(&fixture, "EventStatusFilter", EventStatusFilter::iter());
    assert_values(&fixture, "ForecastInterval", ForecastInterval::iter());
    assert_values(
        &fixture,
        "MultivariateCollectionStatus",
        MultivariateCollectionStatus::iter(),
    );
    assert_values(
        &fixture,
        "IncentiveStatusFilter",
        IncentiveStatusFilter::iter(),
    );
    assert_values(&fixture, "IncentiveTypeFilter", IncentiveTypeFilter::iter());
    assert_values(
        &fixture,
        "PositionSettlementStatus",
        PositionSettlementStatus::iter(),
    );
    assert_values(&fixture, "MarketStatusFilter", MarketStatusFilter::iter());
    assert_values(&fixture, "MveFilter", MveFilter::iter());
    assert_values(&fixture, "MveHistoricalFilter", MveHistoricalFilter::iter());
    assert_values(&fixture, "ExitSlippageUnit", ExitSlippageUnit::iter());
    assert_values(&fixture, "ExitTriggerStatus", ExitTriggerStatus::iter());
    assert_values(&fixture, "ExitTriggerLeg", ExitTriggerLeg::iter());
    assert_values(
        &fixture,
        "FeeType",
        FeeType::iter().filter(|value| !matches!(value, FeeType::Other(_))),
    );
    assert_values(
        &fixture,
        "ExchangeInstance",
        ExchangeInstance::iter().filter(|value| !matches!(value, ExchangeInstance::Other(_))),
    );
    assert_values(&fixture, "UserFilter", UserFilter::iter());
    assert_values(&fixture, "ApiKeyScope", ApiKeyScope::iter());
    assert_values(&fixture, "ApiKeyType", ApiKeyType::iter());
    assert_values(&fixture, "MarketIndexStatus", MarketIndexStatus::iter());
    assert_values(&fixture, "CreditStatus", CreditStatus::iter());
    assert_values(
        &fixture,
        "MarketResult",
        MarketResult::iter().filter(|value| !matches!(value, MarketResult::Other(_))),
    );
    assert_values(&fixture, "PaymentStatus", PaymentStatus::iter());
    assert_values(&fixture, "PaymentType", PaymentType::iter());
    assert_values(&fixture, "OrderAction", OrderAction::iter());
    assert_values(
        &fixture,
        "OrderType",
        OrderType::iter().filter(|value| !matches!(value, OrderType::Other(_))),
    );
    assert_values(&fixture, "IncentiveType", IncentiveType::iter());
    assert_values(
        &fixture,
        "RestingMarginReservation",
        RestingMarginReservation::iter(),
    );
    assert_values(&fixture, "TransferStatus", TransferStatus::iter());
    assert_values(&fixture, "RfqStatus", RfqStatus::iter());
    assert_values(&fixture, "QuoteStatus", QuoteStatus::iter());
    assert_values(&fixture, "RfqBatchStatus", RfqBatchStatus::iter());
    assert_values(&fixture, "RfqBatchQuoteStatus", RfqBatchQuoteStatus::iter());
    assert_values(
        &fixture,
        "MarketType",
        MarketType::iter().filter(|value| !matches!(value, MarketType::Other(_))),
    );
    assert_values(
        &fixture,
        "MarketStatus",
        MarketStatus::iter().filter(|value| !matches!(value, MarketStatus::Other(_))),
    );
    assert_values(
        &fixture,
        "SettlementBoundsType",
        SettlementBoundsType::iter()
            .filter(|value| !matches!(value, SettlementBoundsType::Other(_))),
    );
    assert_values(&fixture, "StrikeType", StrikeType::iter());
    assert_values(&fixture, "OrderUpdateReason", OrderUpdateReason::iter());
    assert_values(
        &fixture,
        "OrderbookUpdateInterval",
        OrderbookUpdateInterval::iter(),
    );
    assert_values(
        &fixture,
        "CfBenchmarksUpdateAction",
        CfBenchmarksUpdateAction::iter(),
    );
    assert_values(&fixture, "PythUpdateAction", PythUpdateAction::iter());
    assert_values(&fixture, "KalshiWsErrorCode", KalshiWsErrorCode::iter());
    assert_values(
        &fixture,
        "MarketLifecycleEvent",
        MarketLifecycleEvent::iter(),
    );
    assert_values(
        &fixture,
        "MultivariateMarketLifecycleEvent",
        MultivariateMarketLifecycleEvent::iter(),
    );
    assert_values(
        &fixture,
        "PriceLevelStructure",
        PriceLevelStructure::iter().filter(|value| !matches!(value, PriceLevelStructure::Other(_))),
    );
    assert_values(
        &fixture,
        "CollateralReturnType",
        CollateralReturnType::iter()
            .filter(|value| !matches!(value, CollateralReturnType::Other(_))),
    );
    assert_values(&fixture, "OrderGroupEvent", OrderGroupEvent::iter());
    assert_values(
        &fixture,
        "ApiUsageTier",
        ApiUsageTier::iter().filter(|value| !matches!(value, ApiUsageTier::Other(_))),
    );
    assert_values(
        &fixture,
        "UsageGrantSource",
        UsageGrantSource::iter().filter(|value| !matches!(value, UsageGrantSource::Other(_))),
    );
    assert_values(&fixture, "PositionCountFilter", PositionCountFilter::iter());
    assert_values(&fixture, "OrderStatusFilter", OrderStatusFilter::iter());
    assert_values(&fixture, "KalshiCommandKind", KalshiCommandKind::iter());
    assert_spelling::<ExitTriggerKind>(&fixture, "ExitTriggerKind");
    assert_spelling::<EventStatusFilter>(&fixture, "EventStatusFilter");
    assert_spelling::<MultivariateCollectionStatus>(&fixture, "MultivariateCollectionStatus");
    assert_spelling::<IncentiveStatusFilter>(&fixture, "IncentiveStatusFilter");
    assert_spelling::<IncentiveTypeFilter>(&fixture, "IncentiveTypeFilter");
    assert_spelling::<PositionSettlementStatus>(&fixture, "PositionSettlementStatus");
    assert_spelling::<MarketStatusFilter>(&fixture, "MarketStatusFilter");
    assert_spelling::<MveFilter>(&fixture, "MveFilter");
    assert_spelling::<MveHistoricalFilter>(&fixture, "MveHistoricalFilter");
    assert_spelling::<ExitSlippageUnit>(&fixture, "ExitSlippageUnit");
    assert_spelling::<ExitTriggerStatus>(&fixture, "ExitTriggerStatus");
    assert_spelling::<ExitTriggerLeg>(&fixture, "ExitTriggerLeg");
    assert_spelling::<FeeType>(&fixture, "FeeType");
    assert_spelling::<ExchangeInstance>(&fixture, "ExchangeInstance");
    assert_spelling::<UserFilter>(&fixture, "UserFilter");
    assert_spelling::<ApiKeyScope>(&fixture, "ApiKeyScope");
    assert_spelling::<ApiKeyType>(&fixture, "ApiKeyType");
    assert_spelling::<MarketIndexStatus>(&fixture, "MarketIndexStatus");
    assert_spelling::<CreditStatus>(&fixture, "CreditStatus");
    assert_spelling::<MarketResult>(&fixture, "MarketResult");
    assert_spelling::<PaymentStatus>(&fixture, "PaymentStatus");
    assert_spelling::<PaymentType>(&fixture, "PaymentType");
    assert_spelling::<OrderAction>(&fixture, "OrderAction");
    assert_spelling::<OrderType>(&fixture, "OrderType");
    assert_spelling::<IncentiveType>(&fixture, "IncentiveType");
    assert_spelling::<RestingMarginReservation>(&fixture, "RestingMarginReservation");
    assert_spelling::<TransferStatus>(&fixture, "TransferStatus");
    assert_spelling::<RfqStatus>(&fixture, "RfqStatus");
    assert_spelling::<QuoteStatus>(&fixture, "QuoteStatus");
    assert_spelling::<RfqBatchStatus>(&fixture, "RfqBatchStatus");
    assert_spelling::<RfqBatchQuoteStatus>(&fixture, "RfqBatchQuoteStatus");
    assert_spelling::<MarketType>(&fixture, "MarketType");
    assert_spelling::<MarketStatus>(&fixture, "MarketStatus");
    assert_spelling::<SettlementBoundsType>(&fixture, "SettlementBoundsType");
    assert_spelling::<StrikeType>(&fixture, "StrikeType");
    assert_spelling::<OrderUpdateReason>(&fixture, "OrderUpdateReason");
    assert_spelling::<CfBenchmarksUpdateAction>(&fixture, "CfBenchmarksUpdateAction");
    assert_spelling::<PythUpdateAction>(&fixture, "PythUpdateAction");
    assert_spelling::<MarketLifecycleEvent>(&fixture, "MarketLifecycleEvent");
    assert_spelling::<MultivariateMarketLifecycleEvent>(
        &fixture,
        "MultivariateMarketLifecycleEvent",
    );
    assert_spelling::<PriceLevelStructure>(&fixture, "PriceLevelStructure");
    assert_spelling::<CollateralReturnType>(&fixture, "CollateralReturnType");
    assert_spelling::<OrderGroupEvent>(&fixture, "OrderGroupEvent");
    assert_spelling::<ApiUsageTier>(&fixture, "ApiUsageTier");
    assert_spelling::<UsageGrantSource>(&fixture, "UsageGrantSource");
    assert_spelling::<PositionCountFilter>(&fixture, "PositionCountFilter");
    assert_spelling::<OrderStatusFilter>(&fixture, "OrderStatusFilter");
    assert_spelling::<KalshiCommandKind>(&fixture, "KalshiCommandKind");
    assert_values(&fixture, "BookSide", BookSide::iter());
    assert_values(&fixture, "OutcomeSide", OutcomeSide::iter());
    assert_values(&fixture, "OrderStatus", OrderStatus::iter());
    assert_values(&fixture, "SelfTradePrevention", SelfTradePrevention::iter());
    assert_values(&fixture, "TimeInForce", TimeInForce::iter());
    assert_values(&fixture, "KalshiChannel", KalshiChannel::iter());
    assert_values(&fixture, "KalshiUpdateAction", KalshiUpdateAction::iter());
}

fn assert_values<T: Serialize + DeserializeOwned + Debug + PartialEq>(
    fixture: &Value,
    name: &str,
    variants: impl Iterator<Item = T>,
) {
    let expected = fixture["enums"][name].as_array().unwrap();
    let variants: Vec<_> = variants.collect();
    assert_eq!(variants.len(), expected.len(), "{name} variant count");

    for variant in variants {
        let wire = serde_json::to_value(&variant).unwrap();
        assert!(
            expected.contains(&wire),
            "undocumented {name} variant {wire}"
        );
        let decoded: T = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, variant, "{name} round trip");
    }

    for wire in expected {
        let decoded: T = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            *wire,
            "{name} wire value"
        );
    }
}

fn assert_spelling<T>(fixture: &Value, name: &str)
where
    T: Serialize
        + DeserializeOwned
        + Debug
        + PartialEq
        + AsRef<str>
        + std::fmt::Display
        + std::str::FromStr,
    T::Err: Debug,
{
    for wire in fixture["enums"][name].as_array().unwrap() {
        let expected: T = serde_json::from_value(wire.clone()).unwrap();
        let text = wire.as_str().unwrap();
        assert_eq!(expected.as_ref(), text, "{name} string reference");
        assert_eq!(expected.to_string(), text, "{name} display");
        assert_eq!(text.parse::<T>().unwrap(), expected, "{name} parsing");
    }
}

#[rstest]
fn schema_inventory_maps_every_allowed_set_to_an_enum() {
    let fixture = fixture();
    let sets = fixture["schema_sets"].as_array().unwrap();
    assert_eq!(sets.len(), 96);

    for entry in sets {
        let name = entry["enum"].as_str().unwrap();
        let values = fixture["enums"][name].as_array().unwrap();

        for value in entry["values"].as_array().unwrap() {
            assert!(
                values.contains(value),
                "{name} omits {value} from {}",
                entry["pointer"]
            );
        }
    }
}

#[rstest]
fn metadata_retains_unknown_labels_without_admitting_them_as_known_variants() {
    assert_unknown(FeeType::Other);
    assert_unknown(ExchangeInstance::Other);
    assert_unknown(MarketResult::Other);
    assert_unknown(OrderType::Other);
    assert_unknown(MarketType::Other);
    assert_unknown(MarketStatus::Other);
    assert_unknown(SettlementBoundsType::Other);
    assert_unknown(PriceLevelStructure::Other);
    assert_unknown(CollateralReturnType::Other);
    assert_unknown(ApiUsageTier::Other);
    assert_unknown(UsageGrantSource::Other);
}

fn assert_unknown<T>(other: fn(String) -> T)
where
    T: Serialize
        + DeserializeOwned
        + Debug
        + PartialEq
        + AsRef<str>
        + std::fmt::Display
        + std::str::FromStr,
    T::Err: Debug,
{
    let wire = Value::String("future_label".into());
    let value: T = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(value, other("future_label".into()));
    assert_eq!(value.as_ref(), "future_label");
    assert_eq!(value.to_string(), "future_label");
    assert_eq!(serde_json::to_value(&value).unwrap(), wire);
    assert_eq!("future_label".parse::<T>().unwrap(), value);
}

#[rstest]
#[case(Value::String("finalized".into()))]
#[case(Value::String("open,closed".into()))]
#[case(Value::String("OPEN".into()))]
#[case(Value::String(String::new()))]
#[case(Value::Null)]
fn market_filters_reject_response_states_and_invalid_values(#[case] value: Value) {
    assert!(serde_json::from_value::<MarketStatusFilter>(value).is_err());
}

#[rstest]
#[case(6)]
#[case(16)]
#[case(17)]
#[case(30)]
#[case(u32::MAX)]
fn websocket_error_codes_reserve_retired_and_unknown_values(#[case] code: u32) {
    assert_eq!(KalshiWsErrorCode::from_repr(code), None);
    assert!(serde_json::from_value::<KalshiWsErrorCode>(code.into()).is_err());
}

#[rstest]
#[case(0)]
#[case(5)]
#[case(30)]
#[case(1441)]
fn candlestick_intervals_reject_unsupported_durations(#[case] minutes: u32) {
    assert!(CandlestickInterval::try_from(minutes).is_err());
    assert!(serde_json::from_value::<CandlestickInterval>(minutes.into()).is_err());
}

#[rstest]
fn position_count_filter_preserves_comma_separated_selection() {
    let query = PositionsQuery {
        count_filter: Some(vec![
            PositionCountFilter::Position,
            PositionCountFilter::TotalTraded,
        ]),
        settlement_status: Some(PositionSettlementStatus::Settled),
        subaccount: 0,
        exchange_index: 3,
        ..Default::default()
    };

    assert_eq!(
        serde_urlencoded::to_string(&query).unwrap(),
        "count_filter=position%2Ctotal_traded&settlement_status=settled&subaccount=0&exchange_index=3"
    );
}

#[rstest]
fn order_status_filter_rejects_unknown_response_status() {
    assert!(serde_json::from_value::<OrderStatusFilter>(Value::String("unknown".into())).is_err());
}

#[rstest]
fn every_documented_data_message_has_a_distinct_variant_and_channel() {
    type Constructor = fn(KalshiDataMessage) -> KalshiWsMessage;

    let constructors: &[(&str, KalshiChannel, Constructor)] = &[
        (
            "orderbook_snapshot",
            KalshiChannel::OrderbookDelta,
            KalshiWsMessage::OrderbookSnapshot,
        ),
        (
            "orderbook_delta",
            KalshiChannel::OrderbookDelta,
            KalshiWsMessage::OrderbookDelta,
        ),
        ("ticker", KalshiChannel::Ticker, KalshiWsMessage::Ticker),
        ("trade", KalshiChannel::Trade, KalshiWsMessage::Trade),
        ("fill", KalshiChannel::Fill, KalshiWsMessage::Fill),
        (
            "market_position",
            KalshiChannel::MarketPositions,
            KalshiWsMessage::MarketPosition,
        ),
        (
            "market_lifecycle_v2",
            KalshiChannel::MarketLifecycleV2,
            KalshiWsMessage::MarketLifecycleV2,
        ),
        (
            "event_lifecycle",
            KalshiChannel::MarketLifecycleV2,
            KalshiWsMessage::EventLifecycle,
        ),
        (
            "event_fee_update",
            KalshiChannel::MarketLifecycleV2,
            KalshiWsMessage::EventFeeUpdate,
        ),
        (
            "user_order",
            KalshiChannel::UserOrders,
            KalshiWsMessage::UserOrder,
        ),
        (
            "multivariate_market_lifecycle",
            KalshiChannel::MultivariateMarketLifecycle,
            KalshiWsMessage::MultivariateMarketLifecycle,
        ),
        (
            "order_group_updates",
            KalshiChannel::OrderGroupUpdates,
            KalshiWsMessage::OrderGroupUpdates,
        ),
        (
            "cfbenchmarks_value",
            KalshiChannel::CfbenchmarksValue,
            KalshiWsMessage::CfbenchmarksValue,
        ),
        (
            "cfbenchmarks_value_indexlist",
            KalshiChannel::CfbenchmarksValue,
            KalshiWsMessage::CfbenchmarksValueIndexlist,
        ),
        (
            "cfbenchmarks_value_5hz",
            KalshiChannel::CfbenchmarksValue5hz,
            KalshiWsMessage::CfbenchmarksValue5hz,
        ),
        (
            "cfbenchmarks_value_5hz_indexlist",
            KalshiChannel::CfbenchmarksValue5hz,
            KalshiWsMessage::CfbenchmarksValue5hzIndexlist,
        ),
        (
            "pyth_value",
            KalshiChannel::PythValue,
            KalshiWsMessage::PythValue,
        ),
        (
            "pyth_value_underlying_list",
            KalshiChannel::PythValue,
            KalshiWsMessage::PythValueUnderlyingList,
        ),
        (
            "rfq_created",
            KalshiChannel::Communications,
            KalshiWsMessage::RfqCreated,
        ),
        (
            "rfq_deleted",
            KalshiChannel::Communications,
            KalshiWsMessage::RfqDeleted,
        ),
        (
            "quote_created",
            KalshiChannel::Communications,
            KalshiWsMessage::QuoteCreated,
        ),
        (
            "quote_accepted",
            KalshiChannel::Communications,
            KalshiWsMessage::QuoteAccepted,
        ),
        (
            "quote_executed",
            KalshiChannel::Communications,
            KalshiWsMessage::QuoteExecuted,
        ),
        (
            "rfq_batch_created",
            KalshiChannel::Communications,
            KalshiWsMessage::RfqBatchCreated,
        ),
        (
            "rfq_batch_deleted",
            KalshiChannel::Communications,
            KalshiWsMessage::RfqBatchDeleted,
        ),
        (
            "batch_quote_created",
            KalshiChannel::Communications,
            KalshiWsMessage::BatchQuoteCreated,
        ),
        (
            "rfq_batch_best_quotes",
            KalshiChannel::Communications,
            KalshiWsMessage::RfqBatchBestQuotes,
        ),
        (
            "batch_quote_accepted",
            KalshiChannel::Communications,
            KalshiWsMessage::BatchQuoteAccepted,
        ),
        (
            "rfq_batch_executed",
            KalshiChannel::Communications,
            KalshiWsMessage::RfqBatchExecuted,
        ),
        (
            "batch_quote_executed",
            KalshiChannel::Communications,
            KalshiWsMessage::BatchQuoteExecuted,
        ),
    ];

    let fixture = fixture();
    let frames = fixture["data_frames"].as_array().unwrap();
    assert_eq!(frames.len(), constructors.len());

    for (kind, channel, constructor) in constructors {
        let frame = frames.iter().find(|frame| frame["type"] == *kind).unwrap();
        let payload: KalshiDataMessage = serde_json::from_value(frame.clone()).unwrap();
        let expected = constructor(payload.clone());
        let decoded = decode_frame(&serde_json::to_vec(frame).unwrap()).unwrap();
        assert_eq!(decoded, expected, "{kind}");
        assert_eq!(decoded.data(), Some((*channel, &payload)), "{kind} channel");
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../test_data/enums.json")).unwrap()
}
