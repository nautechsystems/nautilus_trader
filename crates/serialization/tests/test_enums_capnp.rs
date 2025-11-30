// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Cap'n Proto serialization integration tests for enum types.

#![cfg(feature = "capnp")]

use nautilus_model::enums::{
    AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction,
    BookType, ContingencyType, CurrencyType, InstrumentClass, InstrumentCloseType, LiquiditySide,
    MarketStatusAction, OmsType, OptionKind, OrderSide, OrderStatus, OrderType, PositionSide,
    PriceType, RecordFlag, TimeInForce, TrailingOffsetType, TriggerType,
};
use nautilus_serialization::capnp::conversions::{
    account_type_from_capnp, account_type_to_capnp, aggregation_source_from_capnp,
    aggregation_source_to_capnp, aggressor_side_from_capnp, aggressor_side_to_capnp,
    asset_class_from_capnp, asset_class_to_capnp, bar_aggregation_from_capnp,
    bar_aggregation_to_capnp, book_action_from_capnp, book_action_to_capnp, book_type_from_capnp,
    book_type_to_capnp, contingency_type_from_capnp, contingency_type_to_capnp,
    currency_type_from_capnp, currency_type_to_capnp, instrument_class_from_capnp,
    instrument_class_to_capnp, instrument_close_type_from_capnp, instrument_close_type_to_capnp,
    liquidity_side_from_capnp, liquidity_side_to_capnp, market_status_action_from_capnp,
    market_status_action_to_capnp, oms_type_from_capnp, oms_type_to_capnp, option_kind_from_capnp,
    option_kind_to_capnp, order_side_from_capnp, order_side_to_capnp, order_status_from_capnp,
    order_status_to_capnp, order_type_from_capnp, order_type_to_capnp, position_side_from_capnp,
    position_side_to_capnp, price_type_from_capnp, price_type_to_capnp, record_flag_from_capnp,
    record_flag_to_capnp, time_in_force_from_capnp, time_in_force_to_capnp,
    trailing_offset_type_from_capnp, trailing_offset_type_to_capnp, trigger_type_from_capnp,
    trigger_type_to_capnp,
};
use rstest::rstest;

#[rstest]
#[case(AccountType::Cash)]
#[case(AccountType::Margin)]
#[case(AccountType::Betting)]
fn test_account_type_roundtrip(#[case] value: AccountType) {
    let capnp_value = account_type_to_capnp(value);
    let decoded = account_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(AggressorSide::NoAggressor)]
#[case(AggressorSide::Buyer)]
#[case(AggressorSide::Seller)]
fn test_aggressor_side_roundtrip(#[case] value: AggressorSide) {
    let capnp_value = aggressor_side_to_capnp(value);
    let decoded = aggressor_side_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(AssetClass::FX)]
#[case(AssetClass::Equity)]
#[case(AssetClass::Commodity)]
#[case(AssetClass::Debt)]
#[case(AssetClass::Index)]
#[case(AssetClass::Cryptocurrency)]
#[case(AssetClass::Alternative)]
fn test_asset_class_roundtrip(#[case] value: AssetClass) {
    let capnp_value = asset_class_to_capnp(value);
    let decoded = asset_class_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(InstrumentClass::Spot)]
#[case(InstrumentClass::Swap)]
#[case(InstrumentClass::Future)]
#[case(InstrumentClass::FuturesSpread)]
#[case(InstrumentClass::Forward)]
#[case(InstrumentClass::Cfd)]
#[case(InstrumentClass::Bond)]
#[case(InstrumentClass::Option)]
#[case(InstrumentClass::OptionSpread)]
#[case(InstrumentClass::Warrant)]
#[case(InstrumentClass::SportsBetting)]
#[case(InstrumentClass::BinaryOption)]
fn test_instrument_class_roundtrip(#[case] value: InstrumentClass) {
    let capnp_value = instrument_class_to_capnp(value);
    let decoded = instrument_class_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(OptionKind::Call)]
#[case(OptionKind::Put)]
fn test_option_kind_roundtrip(#[case] value: OptionKind) {
    let capnp_value = option_kind_to_capnp(value);
    let decoded = option_kind_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(OrderSide::NoOrderSide)]
#[case(OrderSide::Buy)]
#[case(OrderSide::Sell)]
fn test_order_side_roundtrip(#[case] value: OrderSide) {
    let capnp_value = order_side_to_capnp(value);
    let decoded = order_side_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(OrderType::Market)]
#[case(OrderType::Limit)]
#[case(OrderType::StopMarket)]
#[case(OrderType::StopLimit)]
#[case(OrderType::MarketToLimit)]
#[case(OrderType::MarketIfTouched)]
#[case(OrderType::LimitIfTouched)]
#[case(OrderType::TrailingStopMarket)]
#[case(OrderType::TrailingStopLimit)]
fn test_order_type_roundtrip(#[case] value: OrderType) {
    let capnp_value = order_type_to_capnp(value);
    let decoded = order_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(OrderStatus::Initialized)]
#[case(OrderStatus::Denied)]
#[case(OrderStatus::Emulated)]
#[case(OrderStatus::Released)]
#[case(OrderStatus::Submitted)]
#[case(OrderStatus::Accepted)]
#[case(OrderStatus::Rejected)]
#[case(OrderStatus::Canceled)]
#[case(OrderStatus::Expired)]
#[case(OrderStatus::Triggered)]
#[case(OrderStatus::PendingUpdate)]
#[case(OrderStatus::PendingCancel)]
#[case(OrderStatus::PartiallyFilled)]
#[case(OrderStatus::Filled)]
fn test_order_status_roundtrip(#[case] value: OrderStatus) {
    let capnp_value = order_status_to_capnp(value);
    let decoded = order_status_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(TimeInForce::Gtc)]
#[case(TimeInForce::Ioc)]
#[case(TimeInForce::Fok)]
#[case(TimeInForce::Gtd)]
#[case(TimeInForce::Day)]
#[case(TimeInForce::AtTheOpen)]
#[case(TimeInForce::AtTheClose)]
fn test_time_in_force_roundtrip(#[case] value: TimeInForce) {
    let capnp_value = time_in_force_to_capnp(value);
    let decoded = time_in_force_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(TriggerType::NoTrigger)]
#[case(TriggerType::Default)]
#[case(TriggerType::LastPrice)]
#[case(TriggerType::MarkPrice)]
#[case(TriggerType::IndexPrice)]
#[case(TriggerType::BidAsk)]
#[case(TriggerType::DoubleLast)]
#[case(TriggerType::DoubleBidAsk)]
#[case(TriggerType::LastOrBidAsk)]
#[case(TriggerType::MidPoint)]
fn test_trigger_type_roundtrip(#[case] value: TriggerType) {
    let capnp_value = trigger_type_to_capnp(value);
    let decoded = trigger_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(ContingencyType::NoContingency)]
#[case(ContingencyType::Oco)]
#[case(ContingencyType::Oto)]
#[case(ContingencyType::Ouo)]
fn test_contingency_type_roundtrip(#[case] value: ContingencyType) {
    let capnp_value = contingency_type_to_capnp(value);
    let decoded = contingency_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(PositionSide::NoPositionSide)]
#[case(PositionSide::Flat)]
#[case(PositionSide::Long)]
#[case(PositionSide::Short)]
fn test_position_side_roundtrip(#[case] value: PositionSide) {
    let capnp_value = position_side_to_capnp(value);
    let decoded = position_side_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(LiquiditySide::NoLiquiditySide)]
#[case(LiquiditySide::Maker)]
#[case(LiquiditySide::Taker)]
fn test_liquidity_side_roundtrip(#[case] value: LiquiditySide) {
    let capnp_value = liquidity_side_to_capnp(value);
    let decoded = liquidity_side_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(BookAction::Add)]
#[case(BookAction::Update)]
#[case(BookAction::Delete)]
#[case(BookAction::Clear)]
fn test_book_action_roundtrip(#[case] value: BookAction) {
    let capnp_value = book_action_to_capnp(value);
    let decoded = book_action_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(BookType::L1_MBP)]
#[case(BookType::L2_MBP)]
#[case(BookType::L3_MBO)]
fn test_book_type_roundtrip(#[case] value: BookType) {
    let capnp_value = book_type_to_capnp(value);
    let decoded = book_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(RecordFlag::F_LAST)]
#[case(RecordFlag::F_TOB)]
#[case(RecordFlag::F_SNAPSHOT)]
#[case(RecordFlag::F_MBP)]
#[case(RecordFlag::RESERVED_2)]
#[case(RecordFlag::RESERVED_1)]
fn test_record_flag_roundtrip(#[case] value: RecordFlag) {
    let capnp_value = record_flag_to_capnp(value);
    let decoded = record_flag_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(AggregationSource::External)]
#[case(AggregationSource::Internal)]
fn test_aggregation_source_roundtrip(#[case] value: AggregationSource) {
    let capnp_value = aggregation_source_to_capnp(value);
    let decoded = aggregation_source_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(PriceType::Bid)]
#[case(PriceType::Ask)]
#[case(PriceType::Mid)]
#[case(PriceType::Last)]
#[case(PriceType::Mark)]
fn test_price_type_roundtrip(#[case] value: PriceType) {
    let capnp_value = price_type_to_capnp(value);
    let decoded = price_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(BarAggregation::Tick)]
#[case(BarAggregation::TickImbalance)]
#[case(BarAggregation::TickRuns)]
#[case(BarAggregation::Volume)]
#[case(BarAggregation::VolumeImbalance)]
#[case(BarAggregation::VolumeRuns)]
#[case(BarAggregation::Value)]
#[case(BarAggregation::ValueImbalance)]
#[case(BarAggregation::ValueRuns)]
#[case(BarAggregation::Millisecond)]
#[case(BarAggregation::Second)]
#[case(BarAggregation::Minute)]
#[case(BarAggregation::Hour)]
#[case(BarAggregation::Day)]
#[case(BarAggregation::Week)]
#[case(BarAggregation::Month)]
#[case(BarAggregation::Year)]
#[case(BarAggregation::Renko)]
fn test_bar_aggregation_roundtrip(#[case] value: BarAggregation) {
    let capnp_value = bar_aggregation_to_capnp(value);
    let decoded = bar_aggregation_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(TrailingOffsetType::NoTrailingOffset)]
#[case(TrailingOffsetType::Price)]
#[case(TrailingOffsetType::BasisPoints)]
#[case(TrailingOffsetType::Ticks)]
#[case(TrailingOffsetType::PriceTier)]
fn test_trailing_offset_type_roundtrip(#[case] value: TrailingOffsetType) {
    let capnp_value = trailing_offset_type_to_capnp(value);
    let decoded = trailing_offset_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(OmsType::Unspecified)]
#[case(OmsType::Netting)]
#[case(OmsType::Hedging)]
fn test_oms_type_roundtrip(#[case] value: OmsType) {
    let capnp_value = oms_type_to_capnp(value);
    let decoded = oms_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(CurrencyType::Crypto)]
#[case(CurrencyType::Fiat)]
#[case(CurrencyType::CommodityBacked)]
fn test_currency_type_roundtrip(#[case] value: CurrencyType) {
    let capnp_value = currency_type_to_capnp(value);
    let decoded = currency_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(InstrumentCloseType::EndOfSession)]
#[case(InstrumentCloseType::ContractExpired)]
fn test_instrument_close_type_roundtrip(#[case] value: InstrumentCloseType) {
    let capnp_value = instrument_close_type_to_capnp(value);
    let decoded = instrument_close_type_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}

#[rstest]
#[case(MarketStatusAction::None)]
#[case(MarketStatusAction::PreOpen)]
#[case(MarketStatusAction::PreCross)]
#[case(MarketStatusAction::Quoting)]
#[case(MarketStatusAction::Cross)]
#[case(MarketStatusAction::Rotation)]
#[case(MarketStatusAction::NewPriceIndication)]
#[case(MarketStatusAction::Trading)]
#[case(MarketStatusAction::Halt)]
#[case(MarketStatusAction::Pause)]
#[case(MarketStatusAction::Suspend)]
#[case(MarketStatusAction::PreClose)]
#[case(MarketStatusAction::Close)]
#[case(MarketStatusAction::PostClose)]
#[case(MarketStatusAction::ShortSellRestrictionChange)]
#[case(MarketStatusAction::NotAvailableForTrading)]
fn test_market_status_action_roundtrip(#[case] value: MarketStatusAction) {
    let capnp_value = market_status_action_to_capnp(value);
    let decoded = market_status_action_from_capnp(capnp_value);
    assert_eq!(value, decoded);
}
