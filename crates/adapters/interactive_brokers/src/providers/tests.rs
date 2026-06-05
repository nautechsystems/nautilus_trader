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

//! Tests for Interactive Brokers instrument provider.

#[cfg(test)]
mod tests {
    use ibapi::contracts::{
        ComboLeg, ComboLegOpenClose, Contract, Currency, Exchange, LegAction, SecurityType, Symbol,
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::AssetClass,
        identifiers::{InstrumentId, Symbol as NautilusSymbol, Venue},
        instruments::{
            Instrument, InstrumentAny, OptionSpread,
            stubs::{audusd_sim, equity_aapl, gbpusd_sim},
        },
        types::{Currency as ModelCurrency, Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use crate::{
        common::parse::create_spread_instrument_id,
        config::InteractiveBrokersInstrumentProviderConfig,
        providers::instruments::InteractiveBrokersInstrumentProvider,
    };

    fn create_test_provider() -> InteractiveBrokersInstrumentProvider {
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        InteractiveBrokersInstrumentProvider::new(config)
    }

    fn create_test_option_spread(instrument_id: InstrumentId) -> OptionSpread {
        OptionSpread::new(
            instrument_id,
            NautilusSymbol::from(instrument_id.symbol.as_str()),
            AssetClass::Equity,
            Some(Ustr::from("XNAS")),
            Ustr::from("SPY"),
            Ustr::from("SPY"),
            UnixNanos::default(),
            UnixNanos::default(),
            ModelCurrency::USD(),
            2,
            Price::from("0.01"),
            Quantity::from(1),
            Quantity::from(1),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    // Note: Tests for load_async, load_with_return_async, load_ids_async, and load_ids_with_return_async
    // require a mock IB client with stubbed contract details responses.
    // These tests should be added when rust-ibapi stubbing capabilities are available.
    // For now, we test the methods that don't require IB client interaction.

    #[rstest]
    fn test_get_instrument_id_by_contract_id() {
        let provider = create_test_provider();
        let contract_id = 265598;

        // Before loading, should return None
        assert!(
            provider
                .get_instrument_id_by_contract_id(contract_id)
                .is_none()
        );
    }

    #[rstest]
    fn test_resolve_instrument_id_for_contract_uses_cached_contract_id() {
        let provider = create_test_provider();
        let instrument = equity_aapl();
        let expected_id = instrument.id();
        provider.insert_test_instrument(InstrumentAny::from(instrument), 265598, 1);
        let contract = Contract {
            contract_id: 265598,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("SMART"),
            currency: Currency::from("USD"),
            ..Default::default()
        };

        let instrument_id = provider
            .resolve_instrument_id_for_contract(&contract)
            .unwrap();

        assert_eq!(instrument_id, expected_id);
    }

    #[rstest]
    fn test_resolve_instrument_id_for_contract_reuses_cached_stock_venue() {
        let provider = create_test_provider();
        let instrument = equity_aapl();
        let expected_id = instrument.id();
        provider.insert_test_instrument(InstrumentAny::from(instrument), 265598, 1);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("SMART"),
            currency: Currency::from("USD"),
            ..Default::default()
        };

        let instrument_id = provider
            .resolve_instrument_id_for_contract(&contract)
            .unwrap();

        assert_eq!(instrument_id, expected_id);
    }

    #[rstest]
    fn test_resolve_instrument_id_for_bag_contract_uses_cached_combo_legs() {
        let provider = create_test_provider();
        let long_leg = InstrumentId::from("SPY C400.SMART");
        let short_leg = InstrumentId::from("SPY C410.SMART");
        let expected_id = create_spread_instrument_id(&[(long_leg, 1), (short_leg, -1)]).unwrap();
        let spread = create_test_option_spread(expected_id);
        provider.insert_test_instrument(InstrumentAny::from(spread), 9000, 1);
        provider.insert_test_contract_id_mapping(1001, long_leg);
        provider.insert_test_contract_id_mapping(1002, short_leg);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("SPY"),
            security_type: SecurityType::Spread,
            exchange: Exchange::from("SMART"),
            currency: Currency::from("USD"),
            combo_legs: vec![
                ComboLeg {
                    contract_id: 1001,
                    ratio: 1,
                    action: LegAction::Buy,
                    exchange: String::from("SMART"),
                    open_close: ComboLegOpenClose::Same,
                    short_sale_slot: 0,
                    designated_location: String::new(),
                    exempt_code: 0,
                },
                ComboLeg {
                    contract_id: 1002,
                    ratio: 1,
                    action: LegAction::Sell,
                    exchange: String::from("SMART"),
                    open_close: ComboLegOpenClose::Same,
                    short_sale_slot: 0,
                    designated_location: String::new(),
                    exempt_code: 0,
                },
            ],
            ..Default::default()
        };

        let instrument_id = provider
            .resolve_instrument_id_for_contract(&contract)
            .unwrap();

        assert_eq!(instrument_id, expected_id);
    }

    #[rstest]
    fn test_instrument_id_to_ib_contract_details() {
        let provider = create_test_provider();
        let instrument_id = InstrumentId::new(NautilusSymbol::from("AAPL"), Venue::from("NASDAQ"));

        // Before loading, should return None
        assert!(
            provider
                .instrument_id_to_ib_contract_details(&instrument_id)
                .is_none()
        );
    }

    #[rstest]
    fn test_find_all_returns_only_requested_cached_instruments() {
        let provider = create_test_provider();
        let audusd = audusd_sim();
        let gbpusd = gbpusd_sim();
        let aapl = equity_aapl();
        let audusd_id = audusd.id();
        let gbpusd_id = gbpusd.id();
        let aapl_id = aapl.id();
        let missing_id = InstrumentId::from("MSFT.NASDAQ");

        provider.insert_test_instrument(InstrumentAny::from(audusd), 1, 1);
        provider.insert_test_instrument(InstrumentAny::from(gbpusd), 2, 1);
        provider.insert_test_instrument(InstrumentAny::from(aapl), 3, 1);

        let instruments = provider.find_all(&[gbpusd_id, missing_id]);
        let instrument_ids: Vec<InstrumentId> = instruments.iter().map(Instrument::id).collect();

        assert_eq!(instrument_ids, vec![gbpusd_id]);
        assert_eq!(provider.count(), 3);
        assert!(provider.find(&audusd_id).is_some());
        assert!(provider.find(&aapl_id).is_some());
    }

    #[rstest]
    fn test_determine_venue() {
        let provider = create_test_provider();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("SMART"),
            primary_exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };

        let venue = provider.determine_venue(&contract, None);
        // Should use primaryExchange when exchange is SMART
        assert_eq!(venue.as_str(), "NASDAQ");
    }

    #[rstest]
    fn test_determine_venue_with_direct_exchange() {
        let provider = create_test_provider();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("SPY"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("ARCA"),
            primary_exchange: Exchange::default(),
            currency: Currency::from("USD"),
            ..Default::default()
        };

        let venue = provider.determine_venue(&contract, None);
        // Should use exchange directly when not SMART
        assert_eq!(venue.as_str(), "ARCA");
    }

    #[rstest]
    fn test_determine_stock_venue_uses_primary_exchange_over_fill_exchange() {
        let provider = create_test_provider();
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("META"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("IBEOS"),
            primary_exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };

        let venue = provider.determine_venue(&contract, None);

        assert_eq!(venue.as_str(), "NASDAQ");
    }

    #[rstest]
    fn test_determine_stock_venue_reuses_compatible_cached_mic_venue() {
        let config = InteractiveBrokersInstrumentProviderConfig {
            convert_exchange_to_mic_venue: true,
            ..Default::default()
        };
        let provider = InteractiveBrokersInstrumentProvider::new(config);
        provider.insert_test_instrument(InstrumentAny::from(equity_aapl()), 265598, 1);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("IBEOS"),
            primary_exchange: Exchange::from("NASDAQ"),
            currency: Currency::from("USD"),
            ..Default::default()
        };

        let venue = provider.determine_venue(&contract, None);

        assert_eq!(venue.as_str(), "XNAS");
    }

    #[rstest]
    fn test_determine_stock_smart_venue_reuses_cached_symbol_venue() {
        let provider = create_test_provider();
        provider.insert_test_instrument(InstrumentAny::from(equity_aapl()), 265598, 1);
        let contract = Contract {
            contract_id: 0,
            symbol: Symbol::from("AAPL"),
            security_type: SecurityType::Stock,
            exchange: Exchange::from("SMART"),
            primary_exchange: Exchange::default(),
            currency: Currency::from("USD"),
            ..Default::default()
        };

        let venue = provider.determine_venue(&contract, None);

        assert_eq!(venue.as_str(), "XNAS");
    }

    #[rstest]
    fn test_symbology_method() {
        let provider = create_test_provider();
        let method = provider.symbology_method();
        // Default should be Simplified
        assert!(matches!(method, crate::config::SymbologyMethod::Simplified));
    }

    #[rstest]
    fn test_get_price_magnifier_defaults_zero_to_one() {
        let provider = create_test_provider();
        let instrument = equity_aapl();
        let instrument_id = instrument.id();

        provider.insert_test_instrument(InstrumentAny::from(instrument), 265598, 0);

        assert_eq!(provider.get_price_magnifier(&instrument_id), 1);
    }

    #[rstest]
    fn test_force_instrument_update_caching() {
        // Test that force_instrument_update parameter affects caching behavior
        // This is tested indirectly through the load methods
        // The actual caching logic is in fetch_contract_details
        let provider = create_test_provider();
        let instrument_id = InstrumentId::new(NautilusSymbol::from("AAPL"), Venue::from("NASDAQ"));

        // Initially, instrument should not be cached
        assert!(provider.find(&instrument_id).is_none());
    }

    #[rstest]
    fn test_is_spread_instrument_id() {
        use crate::common::parse::is_spread_instrument_id;

        // Test spread instrument ID detection
        let spread_id = InstrumentId::new(
            NautilusSymbol::from("(1)AAPL_((2))MSFT"),
            Venue::from("NASDAQ"),
        );
        assert!(is_spread_instrument_id(&spread_id));

        // Test non-spread instrument ID
        let regular_id = InstrumentId::new(NautilusSymbol::from("AAPL"), Venue::from("NASDAQ"));
        assert!(!is_spread_instrument_id(&regular_id));
    }

    #[rstest]
    fn test_parse_spread_instrument_id_to_legs() {
        use crate::common::parse::{
            create_spread_instrument_id, parse_spread_instrument_id_to_legs,
        };

        let leg_tuples = [
            (
                InstrumentId::new(NautilusSymbol::from("MSFT"), Venue::from("NASDAQ")),
                -2,
            ),
            (
                InstrumentId::new(NautilusSymbol::from("AAPL"), Venue::from("NASDAQ")),
                1,
            ),
        ];
        let spread_id = create_spread_instrument_id(&leg_tuples).unwrap();

        assert_eq!(spread_id.symbol.as_str(), "(1)AAPL_((2))MSFT");

        let result = parse_spread_instrument_id_to_legs(&spread_id);
        assert!(result.is_ok());

        let legs = result.unwrap();
        assert_eq!(legs.len(), 2);

        // Check first leg (positive ratio)
        assert_eq!(legs[0].0.symbol.as_str(), "AAPL");
        assert_eq!(legs[0].1, 1);

        // Check second leg (negative ratio)
        assert_eq!(legs[1].0.symbol.as_str(), "MSFT");
        assert_eq!(legs[1].1, -2);
    }
}
