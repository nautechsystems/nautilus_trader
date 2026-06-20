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

//! Example: option-chain backtest from a Tardis-backed catalog.
//!
//! The catalog must already contain option instruments plus per-instrument
//! `QuoteTick` and `OptionGreeks` data, such as data written by the Tardis
//! Machine replay pipeline. Edit `CATALOG_PATH` below to point at it.
//!
//! Edit the constants below to change the catalog, underlying, contract
//! selection, and fee model.
//!
//! Run with:
//! `cargo run -p nautilus-backtest --features examples,streaming --example tardis-option-chain`

use std::{fmt::Debug, path::Path, str::FromStr};

use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType},
    node::BacktestNode,
};
use nautilus_common::actor::DataActor;
use nautilus_core::UnixNanos;
use nautilus_execution::models::fee::{
    CappedOptionFeeModel, FeeModelAny, TieredNotionalOptionFeeModel,
};
use nautilus_model::{
    data::{
        QuoteTick,
        option_chain::{OptionChainSlice, StrikeRange},
    },
    enums::{AccountType, BookType, OmsType, OrderSide, TimeInForce},
    identifiers::{InstrumentId, OptionSeriesId, StrategyId, Venue},
    instruments::InstrumentAny,
    types::{Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore, nautilus_strategy};
use rust_decimal::Decimal;
use ustr::Ustr;

const VENUE: &str = "DERIBIT";
const CATALOG_PATH: &str = "./catalog";
const UNDERLYING: &str = "BTC";
const SELECTION: &str = "delta"; // "delta" or "strike"
const TARGET_DELTA: f64 = 0.25;
const DELTA_TOLERANCE: f64 = 0.05;
const TARGET_STRIKE: Option<&str> = None; // None selects the median strike
const FEE_MODEL: &str = "capped"; // "capped" or "tiered"

#[derive(Debug, Clone)]
struct OptionMetadata {
    instrument_id: InstrumentId,
    underlying: Ustr,
    settlement_currency: Ustr,
    expiration_ns: u64,
    strike: Price,
}

#[derive(Debug, Clone, Copy)]
enum SelectionMode {
    Delta { target: f64, tolerance: f64 },
    Strike { strike: Price },
}

#[derive(Debug, Clone, Copy)]
struct SelectedOption {
    instrument_id: InstrumentId,
    strike: Price,
    quote: QuoteTick,
    delta: Option<f64>,
}

#[derive(Debug)]
struct OptionChainBacktest {
    core: StrategyCore,
    series_id: OptionSeriesId,
    strike_range: StrikeRange,
    selection_mode: SelectionMode,
    trade_size: Quantity,
    orders_submitted: bool,
}

impl OptionChainBacktest {
    fn new(
        series_id: OptionSeriesId,
        strike_range: StrikeRange,
        selection_mode: SelectionMode,
        trade_size: Quantity,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("OPTION-CHAIN-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            series_id,
            strike_range,
            selection_mode,
            trade_size,
            orders_submitted: false,
        }
    }

    fn select_contract(&self, slice: &OptionChainSlice) -> Option<SelectedOption> {
        match self.selection_mode {
            SelectionMode::Delta { target, tolerance } => {
                self.select_by_delta(slice, target, tolerance)
            }
            SelectionMode::Strike { strike } => self.select_by_strike(slice, strike),
        }
    }

    fn select_by_delta(
        &self,
        slice: &OptionChainSlice,
        target: f64,
        tolerance: f64,
    ) -> Option<SelectedOption> {
        let mut best: Option<(f64, SelectedOption)> = None;

        for (strike, data) in slice.calls.iter().chain(slice.puts.iter()) {
            let Some(greeks) = data.greeks.as_ref() else {
                continue;
            };
            let distance = (greeks.delta.abs() - target).abs();
            if distance > tolerance {
                continue;
            }
            let selected = SelectedOption {
                instrument_id: data.quote.instrument_id,
                strike: *strike,
                quote: data.quote,
                delta: Some(greeks.delta),
            };

            if best
                .as_ref()
                .is_none_or(|(best_distance, _)| distance < *best_distance)
            {
                best = Some((distance, selected));
            }
        }

        best.map(|(_, selected)| selected)
            .or_else(|| first_quoted_contract(slice))
    }

    fn select_by_strike(&self, slice: &OptionChainSlice, strike: Price) -> Option<SelectedOption> {
        slice
            .get_call(&strike)
            .or_else(|| slice.get_put(&strike))
            .map(|data| SelectedOption {
                instrument_id: data.quote.instrument_id,
                strike,
                quote: data.quote,
                delta: data.greeks.as_ref().map(|greeks| greeks.delta),
            })
    }

    fn submit_example_orders(&mut self, selected: SelectedOption) -> anyhow::Result<()> {
        log::info!(
            "Selected option {} at strike {} with delta {:?}",
            selected.instrument_id,
            selected.strike,
            selected.delta,
        );

        let trade_size = self.trade_size;
        let maker_order = self.order().limit(
            selected.instrument_id,
            OrderSide::Buy,
            trade_size,
            selected.quote.bid_price,
            Some(TimeInForce::Gtc),
            None,
            Some(true),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(maker_order, None, None, None)?;

        let taker_order = self.order().market(
            selected.instrument_id,
            OrderSide::Buy,
            trade_size,
            Some(TimeInForce::Gtc),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(taker_order, None, None, None)?;

        self.orders_submitted = true;
        Ok(())
    }
}

nautilus_strategy!(OptionChainBacktest);

impl DataActor for OptionChainBacktest {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_option_chain(
            self.series_id,
            self.strike_range.clone(),
            Some(1_000),
            None,
            None,
        );
        Ok(())
    }

    fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
        log::info!(
            "OPTION_CHAIN | {} | atm={:?} | calls={} puts={} strikes={}",
            slice.series_id,
            slice.atm_strike,
            slice.call_count(),
            slice.put_count(),
            slice.strike_count(),
        );

        if self.orders_submitted {
            return Ok(());
        }

        if let Some(selected) = self.select_contract(slice) {
            self.submit_example_orders(selected)?;
        }

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.unsubscribe_option_chain(self.series_id, None);
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    let catalog_path = CATALOG_PATH.to_string();
    let underlying = UNDERLYING.to_string();
    let venue = Venue::new(VENUE);

    let options = load_option_metadata(&catalog_path, venue, &underlying)?;
    let (series_id, instrument_ids, series_options) = nearest_series(venue, &options)?;
    let selection_mode = selection_mode(&series_options)?;
    let strike_range = strike_range(selection_mode);

    let venue_config = BacktestVenueConfig::builder()
        .name(Ustr::from(VENUE))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![starting_balance(series_id.settlement_currency)])
        .fee_model(option_fee_model()?)
        .build();

    let quote_data = BacktestDataConfig::builder()
        .data_type(NautilusDataType::QuoteTick)
        .catalog_path(catalog_path.clone())
        .instrument_ids(instrument_ids.clone())
        .build();
    let greeks_data = BacktestDataConfig::builder()
        .data_type(NautilusDataType::OptionGreeks)
        .catalog_path(catalog_path)
        .instrument_ids(instrument_ids)
        .build();

    let run_config = BacktestRunConfig::builder()
        .id("tardis-option-chain".to_string())
        .venues(vec![venue_config])
        .data(vec![quote_data, greeks_data])
        .chunk_size(10_000)
        .build();
    let config_id = run_config.id().to_string();

    let mut node = BacktestNode::new(vec![run_config])?;
    node.build()?;

    let engine = node
        .get_engine_mut(&config_id)
        .ok_or_else(|| anyhow::anyhow!("Backtest engine not built for run {config_id}"))?;
    engine.add_strategy(OptionChainBacktest::new(
        series_id,
        strike_range,
        selection_mode,
        Quantity::from("1"),
    ))?;

    node.run()?;

    Ok(())
}

fn load_option_metadata(
    catalog_path: &str,
    venue: Venue,
    underlying: &str,
) -> anyhow::Result<Vec<OptionMetadata>> {
    let catalog = ParquetDataCatalog::new(Path::new(catalog_path), None, None, None, None);
    let instruments = catalog.query_instruments(None)?;

    let options: Vec<OptionMetadata> = instruments
        .iter()
        .filter_map(option_metadata)
        .filter(|metadata| {
            metadata.instrument_id.venue == venue
                && metadata
                    .underlying
                    .as_str()
                    .eq_ignore_ascii_case(underlying)
        })
        .collect();

    if options.is_empty() {
        anyhow::bail!(
            "No {underlying} option instruments found for {venue} in catalog {catalog_path}"
        );
    }
    Ok(options)
}

fn nearest_series(
    venue: Venue,
    options: &[OptionMetadata],
) -> anyhow::Result<(OptionSeriesId, Vec<InstrumentId>, Vec<OptionMetadata>)> {
    let expiration_ns = options
        .iter()
        .map(|metadata| metadata.expiration_ns)
        .min()
        .ok_or_else(|| anyhow::anyhow!("No option expirations found"))?;
    let first = options
        .iter()
        .find(|metadata| metadata.expiration_ns == expiration_ns)
        .ok_or_else(|| anyhow::anyhow!("No options found for expiration {expiration_ns}"))?;
    let settlement_currency = options
        .iter()
        .find(|metadata| {
            metadata.expiration_ns == expiration_ns
                && metadata.settlement_currency == metadata.underlying
        })
        .map_or(first.settlement_currency, |metadata| {
            metadata.settlement_currency
        });
    let series_options: Vec<OptionMetadata> = options
        .iter()
        .filter(|metadata| {
            metadata.expiration_ns == expiration_ns
                && metadata.settlement_currency == settlement_currency
        })
        .cloned()
        .collect();
    let instrument_ids = series_options
        .iter()
        .map(|metadata| metadata.instrument_id)
        .collect();
    let series_id = OptionSeriesId::new(
        venue,
        first.underlying,
        settlement_currency,
        UnixNanos::from(expiration_ns),
    );

    Ok((series_id, instrument_ids, series_options))
}

fn option_metadata(instrument: &InstrumentAny) -> Option<OptionMetadata> {
    match instrument {
        InstrumentAny::CryptoOption(option) => Some(OptionMetadata {
            instrument_id: option.id,
            underlying: option.underlying.code,
            settlement_currency: option.settlement_currency.code,
            expiration_ns: option.expiration_ns.as_u64(),
            strike: option.strike_price,
        }),
        InstrumentAny::OptionContract(option) => Some(OptionMetadata {
            instrument_id: option.id,
            underlying: option.underlying,
            settlement_currency: option.currency.code,
            expiration_ns: option.expiration_ns.as_u64(),
            strike: option.strike_price,
        }),
        _ => None,
    }
}

fn selection_mode(options: &[OptionMetadata]) -> anyhow::Result<SelectionMode> {
    match SELECTION {
        "delta" => Ok(SelectionMode::Delta {
            target: TARGET_DELTA,
            tolerance: DELTA_TOLERANCE,
        }),
        "strike" => {
            let strike = match TARGET_STRIKE {
                Some(raw) => Price::from(raw),
                None => median_strike(options)?,
            };
            Ok(SelectionMode::Strike { strike })
        }
        other => {
            anyhow::bail!("Invalid SELECTION '{other}', expected 'delta' or 'strike'")
        }
    }
}

fn strike_range(selection_mode: SelectionMode) -> StrikeRange {
    match selection_mode {
        SelectionMode::Delta { target, tolerance } => StrikeRange::Delta { target, tolerance },
        SelectionMode::Strike { strike } => StrikeRange::Fixed(vec![strike]),
    }
}

fn option_fee_model() -> anyhow::Result<FeeModelAny> {
    match FEE_MODEL {
        "capped" => Ok(FeeModelAny::CappedOption(CappedOptionFeeModel::new(
            Some(parse_decimal("0.0003")?),
            Some(parse_decimal("0.0003")?),
            None,
        )?)),
        "tiered" => Ok(FeeModelAny::TieredNotionalOption(
            TieredNotionalOptionFeeModel::new(
                Some(parse_decimal("0.0002")?),
                Some(parse_decimal("0.0005")?),
            )?,
        )),
        other => {
            anyhow::bail!("Invalid FEE_MODEL '{other}', expected 'capped' or 'tiered'")
        }
    }
}

fn parse_decimal(raw: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str(raw).map_err(Into::into)
}

fn median_strike(options: &[OptionMetadata]) -> anyhow::Result<Price> {
    let mut strikes: Vec<Price> = options.iter().map(|metadata| metadata.strike).collect();
    strikes.sort();
    strikes.dedup();
    strikes
        .get(strikes.len() / 2)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("No strikes found in option metadata"))
}

fn first_quoted_contract(slice: &OptionChainSlice) -> Option<SelectedOption> {
    slice
        .calls
        .iter()
        .chain(slice.puts.iter())
        .next()
        .map(|(strike, data)| SelectedOption {
            instrument_id: data.quote.instrument_id,
            strike: *strike,
            quote: data.quote,
            delta: data.greeks.as_ref().map(|greeks| greeks.delta),
        })
}

fn starting_balance(settlement_currency: Ustr) -> String {
    match settlement_currency.as_str() {
        "BTC" | "ETH" => format!("10 {settlement_currency}"),
        _ => format!("1000000 {settlement_currency}"),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;

    const NEAREST_EXPIRY_NS: u64 = 1_704_067_200_000_000_000;
    const LATER_EXPIRY_NS: u64 = 1_706_745_600_000_000_000;

    fn metadata(
        raw_symbol: &str,
        underlying: &str,
        settlement_currency: &str,
        expiration_ns: u64,
        strike: &str,
    ) -> OptionMetadata {
        OptionMetadata {
            instrument_id: InstrumentId::from(format!("{raw_symbol}.DERIBIT").as_str()),
            underlying: Ustr::from(underlying),
            settlement_currency: Ustr::from(settlement_currency),
            expiration_ns,
            strike: Price::from(strike),
        }
    }

    fn mixed_series_options() -> Vec<OptionMetadata> {
        vec![
            metadata(
                "BTC-20240101-45000-C",
                "BTC",
                "BTC",
                NEAREST_EXPIRY_NS,
                "45000",
            ),
            metadata(
                "BTC-20240101-50000-P",
                "BTC",
                "BTC",
                NEAREST_EXPIRY_NS,
                "50000",
            ),
            metadata("BTC-20240101-1-C", "BTC", "USDC", NEAREST_EXPIRY_NS, "1"),
            metadata("BTC-20240201-2-C", "BTC", "BTC", LATER_EXPIRY_NS, "2"),
        ]
    }

    #[rstest]
    fn test_nearest_series_filters_to_nearest_expiry_and_settlement_currency() {
        let options = mixed_series_options();

        let (series_id, instrument_ids, series_options) =
            nearest_series(Venue::new(VENUE), &options).unwrap();

        assert_eq!(series_id.venue, Venue::new(VENUE));
        assert_eq!(series_id.underlying, Ustr::from("BTC"));
        assert_eq!(series_id.settlement_currency, Ustr::from("BTC"));
        assert_eq!(series_id.expiration_ns, UnixNanos::from(NEAREST_EXPIRY_NS));
        assert_eq!(
            instrument_ids,
            vec![
                InstrumentId::from("BTC-20240101-45000-C.DERIBIT"),
                InstrumentId::from("BTC-20240101-50000-P.DERIBIT"),
            ]
        );
        assert_eq!(series_options.len(), 2);
        assert!(series_options.iter().all(|metadata| {
            metadata.expiration_ns == NEAREST_EXPIRY_NS
                && metadata.settlement_currency == Ustr::from("BTC")
        }));
    }

    #[rstest]
    fn test_default_strike_uses_selected_series_options_only() {
        let options = mixed_series_options();
        let (_series_id, _instrument_ids, series_options) =
            nearest_series(Venue::new(VENUE), &options).unwrap();

        let strike = median_strike(&series_options).unwrap();

        assert_eq!(strike, Price::from("50000"));
        assert!(
            !series_options
                .iter()
                .any(|metadata| metadata.strike == Price::from("1"))
        );
        assert!(
            !series_options
                .iter()
                .any(|metadata| metadata.strike == Price::from("2"))
        );
    }

    #[rstest]
    fn test_strike_range_preserves_delta_and_fixed_modes() {
        assert_eq!(
            strike_range(SelectionMode::Delta {
                target: 0.25,
                tolerance: 0.05,
            }),
            StrikeRange::Delta {
                target: 0.25,
                tolerance: 0.05,
            }
        );
        assert_eq!(
            strike_range(SelectionMode::Strike {
                strike: Price::from("50000"),
            }),
            StrikeRange::Fixed(vec![Price::from("50000")])
        );
    }
}
