// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use nautilus_core::time::UnixNanos;
use pyo3::prelude::*;
use thiserror::Error;

use crate::enums::{AggregationSource, BarAggregation, PriceType};
use crate::identifiers::instrument_id::InstrumentId;
use crate::types::price::Price;
use crate::types::quantity::Quantity;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct BarSpecification {
    pub step: u64,
    pub aggregation: BarAggregation,
    pub price_type: PriceType,
}

impl Display for BarSpecification {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}-{}", self.step, self.aggregation, self.price_type)
    }
}

impl PartialOrd for BarSpecification {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_string().partial_cmp(&other.to_string())
    }

    fn lt(&self, other: &Self) -> bool {
        self.to_string().lt(&other.to_string())
    }

    fn le(&self, other: &Self) -> bool {
        self.to_string().le(&other.to_string())
    }

    fn gt(&self, other: &Self) -> bool {
        self.to_string().gt(&other.to_string())
    }

    fn ge(&self, other: &Self) -> bool {
        self.to_string().ge(&other.to_string())
    }
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct BarType {
    pub instrument_id: InstrumentId,
    pub spec: BarSpecification,
    pub aggregation_source: AggregationSource,
}

#[derive(Debug, Error)]
#[error("Error parsing `BarType` from '{input}', invalid token: '{token}' at position {position}")]
pub struct BarTypeParseError {
    input: String,
    token: String,
    position: usize,
}

impl FromStr for BarType {
    type Err = BarTypeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // TODO: Requires handling some trait related thing
        #[allow(clippy::needless_collect)]
        let pieces: Vec<&str> = s.rsplitn(5, '-').collect();
        let rev_pieces: Vec<&str> = pieces.into_iter().rev().collect();
        if rev_pieces.len() != 5 {
            return Err(BarTypeParseError {
                input: s.to_string(),
                token: "".to_string(),
                position: 0,
            });
        }

        let instrument_id =
            InstrumentId::from_str(rev_pieces[0]).map_err(|_| BarTypeParseError {
                input: s.to_string(),
                token: rev_pieces[0].to_string(),
                position: 0,
            })?;

        let step = rev_pieces[1].parse().map_err(|_| BarTypeParseError {
            input: s.to_string(),
            token: rev_pieces[1].to_string(),
            position: 1,
        })?;
        let aggregation =
            BarAggregation::from_str(rev_pieces[2]).map_err(|_| BarTypeParseError {
                input: s.to_string(),
                token: rev_pieces[2].to_string(),
                position: 2,
            })?;
        let price_type = PriceType::from_str(rev_pieces[3]).map_err(|_| BarTypeParseError {
            input: s.to_string(),
            token: rev_pieces[3].to_string(),
            position: 3,
        })?;
        let aggregation_source =
            AggregationSource::from_str(rev_pieces[4]).map_err(|_| BarTypeParseError {
                input: s.to_string(),
                token: rev_pieces[4].to_string(),
                position: 4,
            })?;

        Ok(BarType {
            instrument_id,
            spec: BarSpecification {
                step,
                aggregation,
                price_type,
            },
            aggregation_source,
        })
    }
}

impl PartialEq for BarType {
    fn eq(&self, other: &Self) -> bool {
        self.instrument_id == other.instrument_id
            && self.spec == other.spec
            && self.aggregation_source == other.aggregation_source
    }
}

impl Hash for BarType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.spec.hash(state);
        self.instrument_id.hash(state);
    }
}

impl PartialOrd for BarType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_string().partial_cmp(&other.to_string())
    }

    fn lt(&self, other: &Self) -> bool {
        self.to_string().lt(&other.to_string())
    }

    fn le(&self, other: &Self) -> bool {
        self.to_string().le(&other.to_string())
    }

    fn gt(&self, other: &Self) -> bool {
        self.to_string().gt(&other.to_string())
    }

    fn ge(&self, other: &Self) -> bool {
        self.to_string().ge(&other.to_string())
    }
}

impl Display for BarType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}-{}-{}",
            self.instrument_id, self.spec, self.aggregation_source
        )
    }
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
#[pyclass]
pub struct Bar {
    pub bar_type: BarType,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Quantity,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl Display for Bar {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.bar_type, self.open, self.high, self.low, self.close, self.volume, self.ts_event
        )
    }
}

#[no_mangle]
pub extern "C" fn bar_new(
    bar_type: BarType,
    open: Price,
    high: Price,
    low: Price,
    close: Price,
    volume: Quantity,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Bar {
    Bar {
        bar_type,
        open,
        high,
        low,
        close,
        volume,
        ts_event,
        ts_init,
    }
}

#[no_mangle]
pub extern "C" fn bar_new_from_raw(
    bar_type: BarType,
    open: i64,
    high: i64,
    low: i64,
    close: i64,
    price_prec: u8,
    volume: u64,
    size_prec: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Bar {
    Bar {
        bar_type,
        open: Price::from_raw(open, price_prec),
        high: Price::from_raw(high, price_prec),
        low: Price::from_raw(low, price_prec),
        close: Price::from_raw(close, price_prec),
        volume: Quantity::from_raw(volume, size_prec),
        ts_event,
        ts_init,
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::BarAggregation;
    use crate::identifiers::symbol::Symbol;
    use crate::identifiers::venue::Venue;

    #[test]
    fn test_bar_spec_equality() {
        let bar_spec1 = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_spec2 = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_spec3 = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Ask,
        };

        assert_eq!(bar_spec1, bar_spec1);
        assert_eq!(bar_spec1, bar_spec2);
        assert_ne!(bar_spec1, bar_spec3);
    }

    #[test]
    fn test_bar_spec_comparison() {
        // # Arrange
        let bar_spec1 = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_spec2 = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_spec3 = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Ask,
        };

        // # Act, Assert
        assert!(bar_spec1 <= bar_spec2);
        assert!(bar_spec3 < bar_spec1);
        assert!(bar_spec1 > bar_spec3);
        assert!(bar_spec1 >= bar_spec3);
    }

    #[test]
    fn test_bar_spec_string_reprs() {
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        assert_eq!(bar_spec.to_string(), "1-MINUTE-BID");
        assert_eq!(format!("{bar_spec}"), "1-MINUTE-BID");
    }

    #[test]
    fn test_bar_type_parse_valid() {
        let input = "BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL";
        let bar_type = BarType::from_str(input).unwrap();

        assert_eq!(
            bar_type.instrument_id,
            InstrumentId::from_str("BTCUSDT-PERP.BINANCE").unwrap()
        );
        assert_eq!(
            bar_type.spec,
            BarSpecification {
                step: 1,
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            }
        );
        assert_eq!(bar_type.aggregation_source, AggregationSource::External);
    }

    #[test]
    fn test_bar_type_parse_invalid_token_pos_0() {
        let input = "BTCUSDT-PERP-1-MINUTE-LAST-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("Error parsing `BarType` from '{input}', invalid token: 'BTCUSDT-PERP' at position 0")
        );
    }

    #[test]
    fn test_bar_type_parse_invalid_token_pos_1() {
        let input = "BTCUSDT-PERP.BINANCE-INVALID-MINUTE-LAST-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 1"
            )
        );
    }

    #[test]
    fn test_bar_type_parse_invalid_token_pos_2() {
        let input = "BTCUSDT-PERP.BINANCE-1-INVALID-LAST-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 2"
            )
        );
    }

    #[test]
    fn test_bar_type_parse_invalid_token_pos_3() {
        let input = "BTCUSDT-PERP.BINANCE-1-MINUTE-INVALID-INTERNAL";
        let result = BarType::from_str(input);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 3"
            )
        );
    }

    #[test]
    fn test_bar_type_parse_invalid_token_pos_4() {
        let input = "BTCUSDT-PERP.BINANCE-1-MINUTE-BID-INVALID";
        let result = BarType::from_str(input);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Error parsing `BarType` from '{input}', invalid token: 'INVALID' at position 4"
            )
        );
    }

    #[test]
    fn test_bar_type_equality() {
        let instrument_id1 = InstrumentId {
            symbol: Symbol::new("AUD/USD"),
            venue: Venue::new("SIM"),
        };
        let instrument_id2 = InstrumentId {
            symbol: Symbol::new("GBP/USD"),
            venue: Venue::new("SIM"),
        };
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_type1 = BarType {
            instrument_id: instrument_id1.clone(),
            spec: bar_spec.clone(),
            aggregation_source: AggregationSource::External,
        };
        let bar_type2 = BarType {
            instrument_id: instrument_id1,
            spec: bar_spec.clone(),
            aggregation_source: AggregationSource::External,
        };
        let bar_type3 = BarType {
            instrument_id: instrument_id2,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        assert_eq!(bar_type1, bar_type1);
        assert_eq!(bar_type1, bar_type2);
        assert_ne!(bar_type1, bar_type3);
    }

    #[test]
    fn test_bar_type_comparison() {
        let instrument_id1 = InstrumentId {
            symbol: Symbol::new("AUD/USD"),
            venue: Venue::new("SIM"),
        };

        let instrument_id2 = InstrumentId {
            symbol: Symbol::new("GBP/USD"),
            venue: Venue::new("SIM"),
        };
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_type1 = BarType {
            instrument_id: instrument_id1.clone(),
            spec: bar_spec.clone(),
            aggregation_source: AggregationSource::External,
        };
        let bar_type2 = BarType {
            instrument_id: instrument_id1,
            spec: bar_spec.clone(),
            aggregation_source: AggregationSource::External,
        };
        let bar_type3 = BarType {
            instrument_id: instrument_id2,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };

        assert!(bar_type1 <= bar_type2);
        assert!(bar_type1 < bar_type3);
        assert!(bar_type3 > bar_type1);
        assert!(bar_type3 >= bar_type1);
    }

    #[test]
    fn test_bar_equality() {
        let instrument_id = InstrumentId {
            symbol: Symbol::new("AUDUSD"),
            venue: Venue::new("SIM"),
        };
        let bar_spec = BarSpecification {
            step: 1,
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Bid,
        };
        let bar_type = BarType {
            instrument_id,
            spec: bar_spec,
            aggregation_source: AggregationSource::External,
        };
        let bar1 = Bar {
            bar_type: bar_type.clone(),
            open: Price::from("1.00001"),
            high: Price::from("1.00004"),
            low: Price::from("1.00002"),
            close: Price::from("1.00003"),
            volume: Quantity::from("100000"),
            ts_event: 0,
            ts_init: 0,
        };

        let bar2 = Bar {
            bar_type,
            open: Price::from("1.00000"),
            high: Price::from("1.00004"),
            low: Price::from("1.00002"),
            close: Price::from("1.00003"),
            volume: Quantity::from("100000"),
            ts_event: 0,
            ts_init: 0,
        };
        assert_eq!(bar1, bar1);
        assert_ne!(bar1, bar2);
    }
}
