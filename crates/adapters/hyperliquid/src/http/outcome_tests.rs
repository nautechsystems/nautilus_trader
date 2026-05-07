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
    use nautilus_model::enums::AssetClass;
    use nautilus_model::instruments::InstrumentAny;
    use rstest::rstest;

    use crate::http::{
        models::{OutcomeDescriptor, OutcomeMetaResponse, OutcomeSideSpec},
        parse::{
            HyperliquidInstrumentDef, HyperliquidMarketType, create_instrument_from_def,
            parse_outcome_instruments,
        },
    };

    #[rstest]
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

    #[rstest]
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
            raw_data: r#"{"outcome":{"outcome":2,"name":"Recurring","description":"class:priceBinary|underlying:BTC|expiry:20260509-0600|targetPrice:80000|period:1d","sideSpecs":[{"name":"Yes"}]}}"#.to_string(),
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
        assert_eq!(binary_option.asset_class, AssetClass::Alternative);
        assert_eq!(binary_option.price_precision, 6);
        assert_eq!(binary_option.size_precision, 6);

        // Check price bounds for prediction markets
        assert!(binary_option.max_price.is_some());
        assert!(binary_option.min_price.is_some());

        // Expiry parsed from encoded description (UTC)
        assert!(binary_option.expiration_ns > 0);
        assert!(binary_option.activation_ns < binary_option.expiration_ns);
    }

    #[rstest]
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
                    name: format!("Test outcome {outcome_id}"),
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
                "outcome_id={outcome_id}, side={side}",
            );
            assert_eq!(
                target_def.raw_symbol.as_str(),
                expected_coin,
                "outcome_id={outcome_id}, side={side}",
            );
        }
    }

    #[rstest]
    fn test_create_binary_option_from_price_bucket_question() {
        let meta = OutcomeMetaResponse {
            outcomes: vec![
                OutcomeDescriptor {
                    outcome: 7130,
                    name: "Recurring Named Outcome".to_string(),
                    description: "index:0".to_string(),
                    side_specs: vec![
                        OutcomeSideSpec {
                            name: "Yes".to_string(),
                        },
                        OutcomeSideSpec {
                            name: "No".to_string(),
                        },
                    ],
                },
                OutcomeDescriptor {
                    outcome: 7131,
                    name: "Recurring Named Outcome".to_string(),
                    description: "index:1".to_string(),
                    side_specs: vec![
                        OutcomeSideSpec {
                            name: "Yes".to_string(),
                        },
                        OutcomeSideSpec {
                            name: "No".to_string(),
                        },
                    ],
                },
                OutcomeDescriptor {
                    outcome: 7132,
                    name: "Recurring Named Outcome".to_string(),
                    description: "index:2".to_string(),
                    side_specs: vec![
                        OutcomeSideSpec {
                            name: "Yes".to_string(),
                        },
                        OutcomeSideSpec {
                            name: "No".to_string(),
                        },
                    ],
                },
            ],
            questions: vec![crate::http::models::OutcomeQuestion {
                question_id: 208,
                name: "Recurring".to_string(),
                description: "class:priceBucket|underlying:BTC|expiry:20260507-1300|priceThresholds:81010,81253|period:15m".to_string(),
                fallback_outcome: Some(7129),
                named_outcomes: vec![7130, 7131, 7132],
                settled_named_outcomes: vec![],
            }],
        };

        let defs = parse_outcome_instruments(&meta).unwrap();
        assert_eq!(defs.len(), 6);

        let yes_def = defs
            .iter()
            .find(|d| d.symbol.as_str() == "OUTCOME-7130-YES-OUTCOME")
            .expect("expected yes def");

        let ts_init = UnixNanos::default();
        let instrument = create_instrument_from_def(yes_def, ts_init).expect("instrument");
        let InstrumentAny::BinaryOption(binary) = instrument else {
            panic!("expected BinaryOption");
        };

        // Activation/expiry must be derived from the question (15m window ending at 13:00 UTC).
        assert!(binary.expiration_ns.as_u64() > 0);
        assert!(binary.activation_ns < binary.expiration_ns);
        assert_eq!(
            (binary.expiration_ns.as_u64() - binary.activation_ns.as_u64()) / 1_000_000_000,
            900
        );

        // Ensure thresholds are exposed on info for downstream paper trading.
        let info = binary.info.expect("expected info");
        let hl = info
            .get("hyperliquid")
            .and_then(|v| v.as_object())
            .expect("expected hyperliquid object");
        let price_bucket = hl
            .get("price_bucket")
            .and_then(|v| v.as_object())
            .expect("expected price_bucket");
        let thresholds = price_bucket
            .get("price_thresholds")
            .and_then(|v| v.as_array())
            .expect("expected price_thresholds");
        assert_eq!(thresholds.len(), 2);
        assert_eq!(thresholds[0].as_str().unwrap(), "81010");
        assert_eq!(thresholds[1].as_str().unwrap(), "81253");
    }
}
