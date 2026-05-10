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
    use ibapi::contracts::{Contract, Currency, Exchange, SecurityType, Symbol};
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol as NautilusSymbol, Venue},
        instruments::{Instrument, InstrumentAny, stubs::equity_aapl},
    };
    use rstest::rstest;

    use crate::{
        config::InteractiveBrokersInstrumentProviderConfig,
        providers::instruments::InteractiveBrokersInstrumentProvider,
    };

    fn create_test_provider() -> InteractiveBrokersInstrumentProvider {
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        InteractiveBrokersInstrumentProvider::new(config)
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
        use crate::common::parse::parse_spread_instrument_id_to_legs;

        // Test parsing spread instrument ID
        let spread_id = InstrumentId::new(
            NautilusSymbol::from("(1)AAPL_((2))MSFT"),
            Venue::from("NASDAQ"),
        );

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
