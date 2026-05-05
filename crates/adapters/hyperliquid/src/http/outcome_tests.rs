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

//! Tests for outcome (prediction) market support.

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::instruments::InstrumentAny;

    use crate::http::{
        models::{OutcomeDescriptor, OutcomeMetaResponse, OutcomeSideSpec},
        parse::{
            HyperliquidInstrumentDef, HyperliquidMarketType, create_instrument_from_def,
            parse_outcome_instruments,
        },
    };

    #[test]
    fn test_parse_outcome_instruments() {
        const OUTCOME_ACTION_ASSET_OFFSET: u32 = 100_000_000;

        let meta = OutcomeMetaResponse {
            outcomes: vec![OutcomeDescriptor {
                outcome: 2,
                name: "BTC above $80k on May 9".to_string(),
                description: "Will BTC be above $80,000 on May 9, 2026?".to_string(),
                side_specs: vec![
                    OutcomeSideSpec {
                        name: "Yes".to_string(),
                    },
                    OutcomeSideSpec {
                        name: "No".to_string(),
                    },
                ],
            }],
            questions: vec![],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();

        // Should create 2 instruments (Yes/No) per outcome
        assert_eq!(defs.len(), 2);

        // Check Yes side (side=0)
        let yes_def = &defs[0];
        assert_eq!(yes_def.symbol.as_str(), "OUTCOME-2-YES-OUTCOME");
        assert_eq!(yes_def.raw_symbol.as_str(), "#20"); // outcome_id * 10 + side
        assert_eq!(yes_def.asset_index, OUTCOME_ACTION_ASSET_OFFSET + 20);
        assert_eq!(yes_def.market_type, HyperliquidMarketType::Outcome);
        assert_eq!(yes_def.quote.as_str(), "USDH");
        assert_eq!(yes_def.max_leverage, Some(1));

        // Check No side (side=1)
        let no_def = &defs[1];
        assert_eq!(no_def.symbol.as_str(), "OUTCOME-2-NO-OUTCOME");
        assert_eq!(no_def.raw_symbol.as_str(), "#21"); // outcome_id * 10 + side
        assert_eq!(no_def.asset_index, OUTCOME_ACTION_ASSET_OFFSET + 21);
        assert_eq!(no_def.market_type, HyperliquidMarketType::Outcome);
    }

    #[test]
    fn test_create_binary_option_from_outcome_def() {
        let def = HyperliquidInstrumentDef {
            symbol: "OUTCOME-2-YES-OUTCOME".into(),
            raw_symbol: "#20".into(),
            base: "BTC above $80k on May 9-Yes".into(),
            quote: "USDH".into(),
            market_type: HyperliquidMarketType::Outcome,
            asset_index: 20,
            price_decimals: 6,
            size_decimals: 6,
            tick_size: rust_decimal::Decimal::new(1, 6),
            lot_size: rust_decimal::Decimal::new(1, 6),
            max_leverage: Some(1),
            only_isolated: false,
            is_hip3: false,
            active: true,
            raw_data: r#"{"outcome":2,"name":"BTC above $80k on May 9","description":"Will BTC be above $80,000?","side_specs":[{"name":"Yes"}]}"#.to_string(),
        };

        let ts_init = UnixNanos::default();
        let instrument = create_instrument_from_def(&def, ts_init);

        assert!(instrument.is_some());

        let InstrumentAny::BinaryOption(binary_option) = instrument.unwrap() else {
            panic!("Expected BinaryOption instrument");
        };

        // Verify BinaryOption properties
        assert_eq!(binary_option.id.symbol.as_str(), "OUTCOME-2-YES-OUTCOME");
        assert_eq!(binary_option.currency.code.as_str(), "USDH");
        assert_eq!(
            binary_option.asset_class,
            nautilus_model::enums::AssetClass::Alternative
        );
        assert_eq!(binary_option.price_precision, 6);
        assert_eq!(binary_option.size_precision, 6);

        // Check price bounds for prediction markets
        assert!(binary_option.max_price.is_some());
        assert!(binary_option.min_price.is_some());
    }

    #[test]
    fn test_outcome_asset_index_encoding() {
        const OUTCOME_ACTION_ASSET_OFFSET: u32 = 100_000_000;

        // Test data-coin encoding + action asset ID encoding
        // data asset: outcome_id * 10 + side
        // action asset: 100_000_000 + data asset
        let test_cases = vec![
            (2, 0, OUTCOME_ACTION_ASSET_OFFSET + 20, "#20"), // outcome 2, Yes side
            (2, 1, OUTCOME_ACTION_ASSET_OFFSET + 21, "#21"), // outcome 2, No side
            (10, 0, OUTCOME_ACTION_ASSET_OFFSET + 100, "#100"), // outcome 10, Yes side
            (10, 1, OUTCOME_ACTION_ASSET_OFFSET + 101, "#101"), // outcome 10, No side
        ];

        for (outcome_id, side, expected_index, expected_coin) in test_cases {
            let meta = OutcomeMetaResponse {
                outcomes: vec![OutcomeDescriptor {
                    outcome: outcome_id,
                    name: format!("Test outcome {}", outcome_id),
                    description: "Test description".to_string(),
                    side_specs: vec![
                        OutcomeSideSpec {
                            name: "Yes".to_string(),
                        },
                        OutcomeSideSpec {
                            name: "No".to_string(),
                        },
                    ],
                }],
                questions: vec![],
            };

            let defs = parse_outcome_instruments(&meta).unwrap();
            let target_def = if side == 0 { &defs[0] } else { &defs[1] };

            assert_eq!(
                target_def.asset_index, expected_index,
                "outcome_id={}, side={}",
                outcome_id, side
            );
            assert_eq!(
                target_def.raw_symbol.as_str(),
                expected_coin,
                "outcome_id={}, side={}",
                outcome_id,
                side
            );
        }
    }
}
