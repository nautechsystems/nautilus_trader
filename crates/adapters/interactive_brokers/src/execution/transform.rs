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

//! Order transformation utilities for converting Nautilus orders to IB orders.

use chrono::{DateTime, Utc};
use ibapi::{
    contracts::Contract,
    orders::{Action, Order as IBOrder, TimeInForce},
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{
        OrderSide, OrderType as NautilusOrderType, TimeInForce as NautilusTimeInForce, TriggerType,
    },
    orders::{Order as NautilusOrder, any::OrderAny},
    types::Price,
};

use crate::{
    common::enums::{IbOrderType, IbTimeInForce, IbTriggerMethod},
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

mod policy;
mod tags;

use self::{
    policy::{
        apply_account_policy, apply_display_quantity_policy, apply_expire_time_policy,
        apply_order_list_policy, apply_quantity_policy, apply_trailing_order_policy,
    },
    tags::apply_ib_order_tags,
};

/// Transform a Nautilus order to an IB order.
///
/// # Errors
///
/// Returns an error if the transformation fails.
pub fn nautilus_order_to_ib_order(
    order: &OrderAny,
    _contract: &Contract,
    instrument_provider: &InteractiveBrokersInstrumentProvider,
    order_id: i32,
    order_ref: &str,
) -> anyhow::Result<IBOrder> {
    let action = match order.order_side() {
        OrderSide::Buy => Action::Buy,
        OrderSide::Sell => Action::Sell,
        _ => anyhow::bail!("Unsupported order side: {:?}", order.order_side()),
    };

    let quantity = order.quantity().as_f64();
    let price_magnifier = instrument_provider.get_price_magnifier(&order.instrument_id()) as f64;

    let (order_type, limit_price, aux_price) = transform_order_type(
        order.order_type(),
        order.time_in_force(),
        order.price(),
        order.trigger_price(),
        price_magnifier,
    );
    let tif = transform_time_in_force(order.time_in_force(), order.expire_time());

    let mut ib_order = IBOrder {
        order_id,
        action,
        total_quantity: quantity,
        order_type: order_type.to_string(),
        limit_price,
        aux_price,
        tif,
        order_ref: order_ref.to_string(),
        account: String::new(),
        ..Default::default()
    };

    apply_expire_time_policy(&mut ib_order, order);
    apply_account_policy(&mut ib_order, order);
    apply_quantity_policy(&mut ib_order, order, instrument_provider);
    apply_trailing_order_policy(&mut ib_order, order, price_magnifier)?;
    apply_display_quantity_policy(&mut ib_order, order);

    // Note: Parent ID in Nautilus is ClientOrderId, but IB expects order_id.
    // Parent order ID mapping requires client_order_id -> IB order_id tracking,
    // which is handled at the execution client layer.
    let _parent_order_id = order.parent_order_id();

    apply_ib_order_tags(&mut ib_order, order.tags())?;
    apply_order_list_policy(&mut ib_order, order);

    Ok(ib_order)
}

/// Transform Nautilus order type to IB order type string and prices.
fn transform_order_type(
    order_type: NautilusOrderType,
    time_in_force: NautilusTimeInForce,
    price: Option<Price>,
    trigger_price: Option<Price>,
    price_magnifier: f64,
) -> (&'static str, Option<f64>, Option<f64>) {
    let ib_order_type = IbOrderType::from_nautilus(order_type, time_in_force);
    let (limit_price, aux_price) = match order_type {
        NautilusOrderType::Market | NautilusOrderType::MarketToLimit => (None, None),
        NautilusOrderType::Limit => (convert_price_opt(price, price_magnifier), None),
        NautilusOrderType::StopMarket | NautilusOrderType::MarketIfTouched => {
            (None, convert_price_opt(trigger_price, price_magnifier))
        }
        NautilusOrderType::StopLimit | NautilusOrderType::LimitIfTouched => (
            convert_price_opt(price, price_magnifier),
            convert_price_opt(trigger_price, price_magnifier),
        ),
        NautilusOrderType::TrailingStopMarket => (None, None),
        NautilusOrderType::TrailingStopLimit => (convert_price_opt(price, price_magnifier), None),
    };

    (ib_order_type.as_str(), limit_price, aux_price)
}

/// Transform Nautilus time in force to IB time in force.
fn transform_time_in_force(
    tif: NautilusTimeInForce,
    _expire_time: Option<nautilus_core::UnixNanos>,
) -> TimeInForce {
    IbTimeInForce::from_nautilus(tif).ibapi_time_in_force()
}

pub(super) fn format_ib_datetime(value: UnixNanos) -> String {
    let dt = DateTime::<Utc>::from(value);
    dt.format("%Y%m%d %H:%M:%S UTC").to_string()
}

pub(super) fn convert_price(price: Price, magnifier: f64) -> f64 {
    price.as_f64() / magnifier
}

fn convert_price_opt(price: Option<Price>, magnifier: f64) -> Option<f64> {
    price.map(|p| convert_price(p, magnifier))
}

pub(super) fn trigger_type_to_ib_trigger_method(
    trigger_type: TriggerType,
) -> ibapi::orders::conditions::TriggerMethod {
    let value = match trigger_type {
        TriggerType::Default => IbTriggerMethod::Default,
        TriggerType::DoubleBidAsk => IbTriggerMethod::DoubleBidAsk,
        TriggerType::LastPrice => IbTriggerMethod::Last,
        TriggerType::DoubleLast => IbTriggerMethod::DoubleLast,
        TriggerType::BidAsk => IbTriggerMethod::BidAsk,
        TriggerType::LastOrBidAsk => IbTriggerMethod::LastOrBidAsk,
        TriggerType::MidPoint => IbTriggerMethod::Midpoint,
        _ => IbTriggerMethod::Default,
    };

    value.ibapi_trigger_method()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use ibapi::{
        contracts::{Contract, Currency, Exchange, SecurityType, Symbol},
        orders::OrderCondition,
    };
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce as NautilusTimeInForce, TrailingOffsetType},
        identifiers::{InstrumentId, OrderListId, Symbol as NautilusSymbol, Venue},
        orders::OrderTestBuilder,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::config::InteractiveBrokersInstrumentProviderConfig;

    fn create_test_order_with_tags(tags_json: &str) -> OrderAny {
        let instrument_id = InstrumentId::new(NautilusSymbol::from("AAPL"), Venue::from("NASDAQ"));

        let tag = Ustr::from(&format!("IBOrderTags:{}", tags_json));
        OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("150.00"))
            .tags(vec![tag])
            .build()
    }

    #[rstest]
    fn test_active_start_time_encoding() {
        let tags_json = r#"{"activeStartTime": "20250101 09:30:00 UTC"}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        let provider = InteractiveBrokersInstrumentProvider::new(config);

        let result = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001");
        assert!(result.is_ok());
        let ib_order = result.unwrap();

        assert_eq!(ib_order.active_start_time, "20250101 09:30:00 UTC");
    }

    #[rstest]
    fn test_active_stop_time_encoding() {
        let tags_json = r#"{"activeStopTime": "20250101 16:00:00 UTC"}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        let provider = InteractiveBrokersInstrumentProvider::new(config);

        let result = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001");
        assert!(result.is_ok());
        let ib_order = result.unwrap();

        assert_eq!(ib_order.active_stop_time, "20250101 16:00:00 UTC");
    }

    #[rstest]
    fn test_both_active_times_encoding() {
        let tags_json = r#"{"activeStartTime": "20250101 09:30:00 UTC", "activeStopTime": "20250101 16:00:00 UTC"}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        let provider = InteractiveBrokersInstrumentProvider::new(config);

        let result = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001");
        assert!(result.is_ok());
        let ib_order = result.unwrap();

        assert_eq!(ib_order.active_start_time, "20250101 09:30:00 UTC");
        assert_eq!(ib_order.active_stop_time, "20250101 16:00:00 UTC");
    }

    #[rstest]
    fn test_at_the_open_maps_to_ib_opg() {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::new(
                NautilusSymbol::from("AAPL"),
                Venue::from("NASDAQ"),
            ))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .time_in_force(NautilusTimeInForce::AtTheOpen)
            .build();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.tif, TimeInForce::OnOpen);
    }

    #[rstest]
    fn test_tags_apply_market_on_open_alias() {
        let tags_json = r#"{"orderType":"MarketOnOpen"}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.order_type, "MKT");
        assert_eq!(ib_order.tif, TimeInForce::OnOpen);
    }

    #[rstest]
    fn test_tags_apply_at_auction_alias() {
        let tags_json = r#"{"orderType":"AtAuction","limitPrice":150.0}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.order_type, "MTL");
        assert_eq!(ib_order.tif, TimeInForce::Auction);
        assert_eq!(ib_order.limit_price, Some(150.0));
    }

    #[rstest]
    fn test_tags_apply_auction_limit_fields() {
        let tags_json = r#"{
            "orderType": "AuctionLimit",
            "auctionStrategy": "Improvement",
            "startingPrice": 1.25,
            "stockRefPrice": 150.25,
            "delta": 0.5,
            "stockRangeLower": 145.0,
            "stockRangeUpper": 155.0
        }"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.order_type, "LMT");
        assert_eq!(
            ib_order.auction_strategy,
            Some(ibapi::orders::AuctionStrategy::Improvement)
        );
        assert_eq!(ib_order.starting_price, Some(1.25));
        assert_eq!(ib_order.stock_ref_price, Some(150.25));
        assert_eq!(ib_order.delta, Some(0.5));
        assert_eq!(ib_order.stock_range_lower, Some(145.0));
        assert_eq!(ib_order.stock_range_upper, Some(155.0));
    }

    #[rstest]
    fn test_tags_apply_auction_relative_fields() {
        let tags_json = r#"{"orderType":"AuctionRelative","auxPrice":0.01}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.order_type, "REL");
        assert_eq!(ib_order.aux_price, Some(0.01));
    }

    #[rstest]
    fn test_tags_apply_generic_ib_order_fields() {
        let tags_json = r#"{
            "displaySize": 25,
            "triggerMethod": 2,
            "overridePercentageConstraints": true,
            "rule80A": "A",
            "openClose": "O",
            "origin": 1,
            "shortSaleSlot": 2,
            "designatedLocation": "SLB",
            "discretionaryAmt": 0.12,
            "optOutSmartRouting": true,
            "volatility": 23.5,
            "volatilityType": 2,
            "continuousUpdate": true,
            "referencePriceType": 2,
            "deltaNeutralOrderType": "MKT",
            "deltaNeutralAuxPrice": 1.25,
            "scaleInitLevelSize": 10,
            "scaleAutoReset": true,
            "hedgeType": "D",
            "hedgeParam": "0.5",
            "algoStrategy": "Adaptive",
            "algoParams": [{"tag": "adaptivePriority", "value": "Normal"}],
            "notHeld": true,
            "cashQty": 1000.0,
            "mifid2DecisionMaker": "maker",
            "autoCancelParent": true,
            "minTradeQty": 5,
            "competeAgainstBestOffset": 0.01,
            "midOffsetAtWhole": 0.02,
            "referenceContractId": 123,
            "referenceExchange": "SMART",
            "adjustedOrderType": "STP",
            "triggerPrice": 149.0,
            "conditionsIgnoreRth": true,
            "usePriceMgmtAlgo": true,
            "duration": 30,
            "postToAts": 10,
            "includeOvernight": true,
            "manualOrderIndicator": 1,
            "submitter": "SUB",
            "NonGuaranteed": true,
            "orderComboLegs": [{"price": 1.23}],
            "softDollarTier": {"name": "tier", "value": "val", "display_name": "display"}
        }"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.display_size, Some(25));
        assert_eq!(
            ib_order.trigger_method,
            ibapi::orders::conditions::TriggerMethod::Last
        );
        assert!(ib_order.override_percentage_constraints);
        assert_eq!(ib_order.rule_80_a, Some(ibapi::orders::Rule80A::Agency));
        assert_eq!(
            ib_order.open_close,
            Some(ibapi::orders::OrderOpenClose::Open)
        );
        assert_eq!(ib_order.origin, ibapi::orders::OrderOrigin::Firm);
        assert_eq!(
            ib_order.short_sale_slot,
            ibapi::orders::ShortSaleSlot::ThirdParty
        );
        assert_eq!(ib_order.designated_location, "SLB");
        assert_eq!(ib_order.discretionary_amt, 0.12);
        assert!(ib_order.opt_out_smart_routing);
        assert_eq!(ib_order.volatility, Some(23.5));
        assert_eq!(
            ib_order.volatility_type,
            Some(ibapi::orders::VolatilityType::Annual)
        );
        assert!(ib_order.continuous_update);
        assert_eq!(
            ib_order.reference_price_type,
            Some(ibapi::orders::ReferencePriceType::NBBO)
        );
        assert_eq!(ib_order.delta_neutral_order_type, "MKT");
        assert_eq!(ib_order.delta_neutral_aux_price, Some(1.25));
        assert_eq!(ib_order.scale_init_level_size, Some(10));
        assert!(ib_order.scale_auto_reset);
        assert_eq!(ib_order.hedge_type, "D");
        assert_eq!(ib_order.hedge_param, "0.5");
        assert_eq!(ib_order.algo_strategy, "Adaptive");
        assert_eq!(ib_order.algo_params[0].tag, "adaptivePriority");
        assert_eq!(ib_order.algo_params[0].value, "Normal");
        assert!(ib_order.not_held);
        assert_eq!(ib_order.cash_qty, Some(1000.0));
        assert_eq!(ib_order.mifid2_decision_maker, "maker");
        assert!(ib_order.auto_cancel_parent);
        assert_eq!(ib_order.min_trade_qty, Some(5));
        assert_eq!(ib_order.compete_against_best_offset, Some(0.01));
        assert_eq!(ib_order.mid_offset_at_whole, Some(0.02));
        assert_eq!(ib_order.reference_contract_id, 123);
        assert_eq!(ib_order.reference_exchange, "SMART");
        assert_eq!(ib_order.adjusted_order_type, "STP");
        assert_eq!(ib_order.trigger_price, Some(149.0));
        assert!(ib_order.conditions_ignore_rth);
        assert!(ib_order.use_price_mgmt_algo);
        assert_eq!(ib_order.duration, Some(30));
        assert_eq!(ib_order.post_to_ats, Some(10));
        assert!(ib_order.include_overnight);
        assert_eq!(ib_order.manual_order_indicator, Some(1));
        assert_eq!(ib_order.submitter, "SUB");
        assert_eq!(ib_order.order_combo_legs[0].price, Some(1.23));
        assert_eq!(ib_order.soft_dollar_tier.name, "tier");
        assert_eq!(ib_order.soft_dollar_tier.value, "val");
        assert_eq!(ib_order.soft_dollar_tier.display_name, "display");
        assert!(
            ib_order
                .smart_combo_routing_params
                .iter()
                .any(|tag| tag.tag == "NonGuaranteed" && tag.value == "1")
        );
    }

    #[rstest]
    fn test_invalid_tag_set_rejects_order_transform() {
        let tags_json = r#"{"whatIf": true, "displaySize": "invalid"}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let result = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001");

        assert!(result.is_err());
        assert!(
            result
                .expect_err("invalid tag set should reject the order")
                .to_string()
                .contains("Invalid IBOrderTags field display_size")
        );
    }

    #[rstest]
    fn test_non_utc_datetime_tag_rejects_order_transform() {
        let tags_json = r#"{"activeStartTime": "20250101 09:30:00 EST"}"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let result = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001");

        assert!(result.is_err());
        assert!(
            result
                .expect_err("non-UTC datetime tag should reject the order")
                .to_string()
                .contains("Invalid IBOrderTags field active_start_time")
        );
    }

    #[rstest]
    fn test_gtd_orders_encode_ib_timestamp_string() {
        let expire_time = UnixNanos::from(
            Utc.with_ymd_and_hms(2025, 1, 15, 14, 30, 0)
                .single()
                .expect("valid datetime"),
        );
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::new(
                NautilusSymbol::from("AAPL"),
                Venue::from("NASDAQ"),
            ))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("150.00"))
            .time_in_force(NautilusTimeInForce::Gtd)
            .expire_time(expire_time)
            .build();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.tif, TimeInForce::GoodTilDate);
        assert_eq!(ib_order.good_till_date, "20250115 14:30:00 UTC");
    }

    #[rstest]
    fn test_trailing_stop_market_uses_aux_price_not_trailing_percent() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id(InstrumentId::new(
                NautilusSymbol::from("AAPL"),
                Venue::from("NASDAQ"),
            ))
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100))
            .trigger_price(Price::from("149.50"))
            .trailing_offset(dec!(0.5))
            .trailing_offset_type(TrailingOffsetType::Price)
            .build();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.aux_price, Some(0.5));
        assert_eq!(ib_order.trail_stop_price, Some(149.5));
        assert_eq!(ib_order.trailing_percent, None);
    }

    #[rstest]
    fn test_trailing_stop_rejects_non_price_offset() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
            .instrument_id(InstrumentId::new(
                NautilusSymbol::from("AAPL"),
                Venue::from("NASDAQ"),
            ))
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100))
            .trigger_price(Price::from("149.50"))
            .trailing_offset(dec!(0.5))
            .trailing_offset_type(TrailingOffsetType::BasisPoints)
            .build();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let result = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001");

        assert!(result.is_err());
        assert!(
            result
                .expect_err("transform should reject unsupported trailing offset type")
                .to_string()
                .contains("only PRICE is supported")
        );
    }

    #[rstest]
    fn test_tags_apply_conditions_and_cancel_order_policy() {
        let tags_json = r#"{
            "outsideRth": true,
            "whatIf": true,
            "conditionsCancelOrder": true,
            "conditions": [
                {
                    "type": "price",
                    "conId": 265598,
                    "exchange": "SMART",
                    "price": 150.0,
                    "isMore": true,
                    "triggerMethod": 2,
                    "conjunction": "and"
                },
                {
                    "type": "time",
                    "time": "20251230 14:30:00 US/Eastern",
                    "isMore": false,
                    "conjunction": "or"
                }
            ]
        }"#;
        let order = create_test_order_with_tags(tags_json);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert!(ib_order.outside_rth);
        assert!(ib_order.what_if);
        assert!(ib_order.conditions_cancel_order);
        assert_eq!(ib_order.conditions.len(), 2);
        match &ib_order.conditions[0] {
            OrderCondition::Price(condition) => {
                assert_eq!(condition.contract_id, 265598);
                assert_eq!(condition.exchange, "SMART");
                assert_eq!(condition.price, 150.0);
                assert!(condition.is_more);
                assert!(condition.is_conjunction);
            }
            other => panic!("unexpected first condition: {other:?}"),
        }

        match &ib_order.conditions[1] {
            OrderCondition::Time(condition) => {
                assert_eq!(condition.time, "20251230 14:30:00 US/Eastern");
                assert!(!condition.is_more);
                assert!(!condition.is_conjunction);
            }
            other => panic!("unexpected second condition: {other:?}"),
        }
    }

    #[rstest]
    fn test_order_list_id_sets_oca_group_when_missing() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::new(
                NautilusSymbol::from("AAPL"),
                Venue::from("NASDAQ"),
            ))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("150.00"))
            .order_list_id(OrderListId::from("OL-001"))
            .build();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.oca_group, "OL-001");
    }

    #[rstest]
    fn test_explicit_oca_group_tag_overrides_order_list_default() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::new(
                NautilusSymbol::from("AAPL"),
                Venue::from("NASDAQ"),
            ))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("150.00"))
            .order_list_id(OrderListId::from("OL-001"))
            .tags(vec![Ustr::from(
                r#"IBOrderTags:{"ocaGroup":"CUSTOM-GROUP","ocaType":1}"#,
            )])
            .build();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        );

        let ib_order = nautilus_order_to_ib_order(&order, &contract, &provider, 1, "TEST-001")
            .expect("order transform should succeed");

        assert_eq!(ib_order.oca_group, "CUSTOM-GROUP");
        assert_eq!(ib_order.oca_type, ibapi::orders::OcaType::from(1));
    }
}
