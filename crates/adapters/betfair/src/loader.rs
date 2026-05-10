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

//! File-based loader for historical Betfair Exchange Streaming data.
//!
//! Reads compressed (gzip or bzip2) or plain JSON files containing
//! newline-delimited Betfair ESA messages and produces Nautilus domain
//! objects. The parsing logic mirrors the live data client handler in
//! [`crate::data`].

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use ahash::AHashMap;
use anyhow::Context;
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use nautilus_model::{
    data::{InstrumentClose, InstrumentStatus, OrderBookDeltas, TradeTick},
    identifiers::{InstrumentId, TradeId},
    instruments::{Instrument, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{BETFAIR_PRICE_PRECISION, BETFAIR_QUANTITY_PRECISION},
        enums::MarketStatus,
        parse::{make_instrument_id, parse_market_definition, parse_millis_timestamp},
    },
    data_types::{
        BetfairBspBookDelta, BetfairRaceProgress, BetfairRaceRunnerData, BetfairSequenceCompleted,
        BetfairStartingPrice, BetfairTicker,
    },
    stream::{
        messages::{MCM, RCM, StreamMessage, stream_decode},
        parse::{
            make_trade_tick, parse_betfair_starting_prices, parse_betfair_ticker,
            parse_bsp_book_deltas, parse_instrument_closes, parse_instrument_statuses,
            parse_race_progress, parse_race_runner_data, parse_runner_book_deltas,
        },
    },
};

/// A parsed data item from a Betfair historical file.
#[derive(Debug)]
pub enum BetfairDataItem {
    /// Instrument definition from a market definition.
    Instrument(Box<InstrumentAny>),
    /// Market status change for an instrument.
    Status(InstrumentStatus),
    /// Order book snapshot or delta update.
    Deltas(OrderBookDeltas),
    /// Incremental trade tick derived from cumulative traded volumes.
    Trade(TradeTick),
    /// Betfair-specific ticker data (last traded price, traded volume, BSP near/far).
    Ticker(BetfairTicker),
    /// Betfair Starting Price for a runner.
    StartingPrice(BetfairStartingPrice),
    /// BSP book delta (separate from exchange book).
    BspBookDelta(BetfairBspBookDelta),
    /// Instrument close event at market settlement.
    InstrumentClose(InstrumentClose),
    /// Marker emitted after each MCM batch is fully processed.
    SequenceCompleted(BetfairSequenceCompleted),
    /// GPS tracking data for a race runner (from RCM).
    RaceRunnerData(BetfairRaceRunnerData),
    /// Race-level progress data (from RCM).
    RaceProgress(BetfairRaceProgress),
}

/// Reads Betfair historical data files and converts them into Nautilus domain objects.
///
/// Each file contains newline-delimited JSON from the Betfair Exchange Streaming API.
/// The loader handles gzip decompression, stateful traded volume tracking, and
/// instrument creation from market definitions.
#[derive(Debug)]
pub struct BetfairDataLoader {
    currency: Currency,
    min_notional: Option<Money>,
    traded_volumes: AHashMap<(InstrumentId, Decimal), Decimal>,
    instruments: AHashMap<InstrumentId, InstrumentAny>,
}

impl BetfairDataLoader {
    /// Creates a new [`BetfairDataLoader`].
    #[must_use]
    pub fn new(currency: Currency, min_notional: Option<Money>) -> Self {
        Self {
            currency,
            min_notional,
            traded_volumes: AHashMap::new(),
            instruments: AHashMap::new(),
        }
    }

    /// Returns the instruments cached from the most recent load.
    #[must_use]
    pub fn instruments(&self) -> &AHashMap<InstrumentId, InstrumentAny> {
        &self.instruments
    }

    /// Clears all cached state (instruments and traded volumes).
    pub fn reset(&mut self) {
        self.traded_volumes.clear();
        self.instruments.clear();
    }

    /// Loads a Betfair historical data file and returns all parsed data items.
    ///
    /// Supports gzip-compressed (`.gz`), bzip2-compressed (`.bz2`), and plain JSON files.
    /// Each line is deserialized as a Betfair stream message. MCM and RCM
    /// messages are parsed into Nautilus domain objects; other message types
    /// are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or a line fails to parse.
    pub fn load(&mut self, filepath: &Path) -> anyhow::Result<Vec<BetfairDataItem>> {
        let reader = open_reader(filepath)?;
        let mut items = Vec::new();

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.with_context(|| {
                format!(
                    "failed to read line {} of '{}'",
                    line_num + 1,
                    filepath.display()
                )
            })?;

            if line.is_empty() {
                continue;
            }

            let msg = match stream_decode(line.as_bytes()) {
                Ok(msg) => msg,
                Err(e) => {
                    log::warn!("Failed to decode line {}: {e}", line_num + 1);
                    continue;
                }
            };

            match msg {
                StreamMessage::MarketChange(mcm) => self.process_mcm(&mcm, &mut items),
                StreamMessage::RaceChange(rcm) => Self::process_rcm(&rcm, &mut items),
                StreamMessage::Connection(_)
                | StreamMessage::Status(_)
                | StreamMessage::OrderChange(_) => {}
            }
        }

        Ok(items)
    }

    /// Loads only instrument definitions from a Betfair historical data file.
    ///
    /// Scans the file for market definitions and creates instruments, but
    /// skips all other data processing. Faster than `load()` when only
    /// instruments are needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or parsed.
    pub fn load_instruments(&mut self, filepath: &Path) -> anyhow::Result<Vec<InstrumentAny>> {
        let reader = open_reader(filepath)?;

        for line_result in reader.lines() {
            let line = line_result?;
            if line.is_empty() {
                continue;
            }

            let msg = match stream_decode(line.as_bytes()) {
                Ok(msg) => msg,
                Err(_) => continue,
            };

            if let StreamMessage::MarketChange(mcm) = msg {
                let Some(market_changes) = &mcm.mc else {
                    continue;
                };

                let ts_init = parse_millis_timestamp(mcm.pt);

                for mc in market_changes {
                    if let Some(def) = &mc.market_definition
                        && let Ok(instruments) = parse_market_definition(
                            &mc.id,
                            def,
                            self.currency,
                            ts_init,
                            self.min_notional,
                        )
                    {
                        for inst in instruments {
                            self.instruments.insert(inst.id(), inst);
                        }
                    }
                }
            }
        }

        Ok(self.instruments.values().cloned().collect())
    }

    fn process_mcm(&mut self, mcm: &MCM, items: &mut Vec<BetfairDataItem>) {
        if mcm.is_heartbeat() {
            return;
        }

        let Some(market_changes) = &mcm.mc else {
            return;
        };

        let ts_event = parse_millis_timestamp(mcm.pt);
        let ts_init = ts_event;

        for mc in market_changes {
            let is_snapshot = mc.img;
            let mut market_closed = false;

            if let Some(def) = &mc.market_definition {
                // Emit instruments first so sequential consumers (e.g. the backtest
                // exchange) have the instrument in cache before any status or close
                // event references it.
                match parse_market_definition(
                    &mc.id,
                    def,
                    self.currency,
                    ts_init,
                    self.min_notional,
                ) {
                    Ok(new_instruments) => {
                        for inst in &new_instruments {
                            self.instruments.insert(inst.id(), inst.clone());
                        }

                        for inst in new_instruments {
                            items.push(BetfairDataItem::Instrument(Box::new(inst)));
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to parse market definition for {}: {e}", mc.id);
                    }
                }

                if let Some(status) = &def.status {
                    market_closed = *status == MarketStatus::Closed;

                    for event in parse_instrument_statuses(&mc.id, def, ts_event, ts_init) {
                        items.push(BetfairDataItem::Status(event));
                    }
                }

                for sp in parse_betfair_starting_prices(&mc.id, def, ts_event, ts_init) {
                    items.push(BetfairDataItem::StartingPrice(sp));
                }

                for close in parse_instrument_closes(&mc.id, def, ts_event, ts_init) {
                    items.push(BetfairDataItem::InstrumentClose(close));
                }
            }

            // Non-snapshot deltas and BSP deltas are buffered and flushed after
            // trades/tickers to mirror the Python `market_change_to_updates`
            // ordering (book deltas first, then BSP). Snapshots go inline per
            // runner, also matching Python.
            let mut buffered_deltas: Vec<OrderBookDeltas> = Vec::new();
            let mut buffered_bsp_deltas: Vec<BetfairBspBookDelta> = Vec::new();

            if let Some(runner_changes) = &mc.rc {
                for rc in runner_changes {
                    let handicap = rc.hc.unwrap_or(Decimal::ZERO);
                    let instrument_id = make_instrument_id(&mc.id, rc.id, handicap);

                    match parse_runner_book_deltas(
                        instrument_id,
                        rc,
                        is_snapshot,
                        mcm.pt,
                        ts_event,
                        ts_init,
                    ) {
                        Ok(Some(deltas)) => {
                            if is_snapshot {
                                items.push(BetfairDataItem::Deltas(deltas));
                            } else {
                                buffered_deltas.push(deltas);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!("Failed to parse book deltas for {instrument_id}: {e}");
                        }
                    }

                    if let Some(trades) = &rc.trd {
                        for pv in trades {
                            if pv.volume == Decimal::ZERO {
                                continue;
                            }

                            let key = (instrument_id, pv.price);
                            let prev_volume = self
                                .traded_volumes
                                .get(&key)
                                .copied()
                                .unwrap_or(Decimal::ZERO);

                            if pv.volume <= prev_volume {
                                continue;
                            }

                            let trade_volume = pv.volume - prev_volume;
                            self.traded_volumes.insert(key, pv.volume);

                            let price =
                                match Price::from_decimal_dp(pv.price, BETFAIR_PRICE_PRECISION) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        log::warn!("Invalid trade price: {e}");
                                        continue;
                                    }
                                };
                            let size = match Quantity::from_decimal_dp(
                                trade_volume,
                                BETFAIR_QUANTITY_PRECISION,
                            ) {
                                Ok(q) => q,
                                Err(e) => {
                                    log::warn!("Invalid trade size: {e}");
                                    continue;
                                }
                            };
                            let trade_id =
                                TradeId::new(format!("{}-{}-{}", mcm.pt, rc.id, pv.price));
                            let tick = make_trade_tick(
                                instrument_id,
                                price,
                                size,
                                trade_id,
                                ts_event,
                                ts_init,
                            );
                            items.push(BetfairDataItem::Trade(tick));
                        }
                    }

                    if let Some(ticker) = parse_betfair_ticker(instrument_id, rc, ts_event, ts_init)
                    {
                        items.push(BetfairDataItem::Ticker(ticker));
                    }

                    buffered_bsp_deltas.extend(parse_bsp_book_deltas(
                        instrument_id,
                        rc,
                        ts_event,
                        ts_init,
                    ));
                }
            }

            for deltas in buffered_deltas {
                items.push(BetfairDataItem::Deltas(deltas));
            }

            for bsp_delta in buffered_bsp_deltas {
                items.push(BetfairDataItem::BspBookDelta(bsp_delta));
            }

            if market_closed {
                let prefix = format!("{}-", mc.id);
                self.traded_volumes
                    .retain(|k, _| !k.0.symbol.as_str().starts_with(&prefix));
            }
        }

        items.push(BetfairDataItem::SequenceCompleted(
            BetfairSequenceCompleted::new(ts_event, ts_init),
        ));
    }

    fn process_rcm(rcm: &RCM, items: &mut Vec<BetfairDataItem>) {
        let Some(race_changes) = &rcm.rc else {
            return;
        };

        let fallback_ts = parse_millis_timestamp(rcm.pt);

        for rc in race_changes {
            let race_id = rc.id.as_deref().unwrap_or("");
            let market_id = rc.mid.as_deref().unwrap_or("");

            if let Some(runners) = &rc.rrc {
                for rrc in runners {
                    let ts_event = rrc.ft.map_or(fallback_ts, parse_millis_timestamp);

                    if let Some(runner) =
                        parse_race_runner_data(race_id, market_id, rrc, ts_event, ts_event)
                    {
                        items.push(BetfairDataItem::RaceRunnerData(runner));
                    }
                }
            }

            if let Some(rpc) = &rc.rpc {
                let ts_event = rpc.ft.map_or(fallback_ts, parse_millis_timestamp);
                let progress = parse_race_progress(race_id, market_id, rpc, ts_event, ts_event);
                items.push(BetfairDataItem::RaceProgress(progress));
            }
        }
    }
}

fn open_reader(filepath: &Path) -> anyhow::Result<Box<dyn BufRead>> {
    let file =
        File::open(filepath).with_context(|| format!("failed to open '{}'", filepath.display()))?;

    let ext = filepath.extension().and_then(|e| e.to_str()).unwrap_or("");

    if ext.eq_ignore_ascii_case("gz") {
        Ok(Box::new(BufReader::new(GzDecoder::new(file))))
    } else if ext.eq_ignore_ascii_case("bz2") {
        Ok(Box::new(BufReader::new(BzDecoder::new(file))))
    } else {
        Ok(Box::new(BufReader::new(file)))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    fn compact_json(pretty: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(pretty).unwrap();
        serde_json::to_string(&value).unwrap()
    }

    fn local_data_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap()
            .join("tests/test_data/local/betfair")
    }

    fn test_data_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
    }

    #[rstest]
    fn test_load_bz2_file() {
        let filepath = test_data_dir().join("stream/sample.bz2");
        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&filepath).unwrap();

        let instrument_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Instrument(_)))
            .count();
        assert!(
            instrument_count > 0,
            "should parse instruments from bz2 file"
        );
        assert_eq!(loader.instruments().len(), instrument_count);

        let has_sequence = items
            .iter()
            .any(|i| matches!(i, BetfairDataItem::SequenceCompleted(_)));
        assert!(has_sequence, "should emit SequenceCompleted");
    }

    #[rstest]
    fn test_load_single_mcm_line() {
        let data = compact_json(&load_test_json("stream/mcm_SUB_IMAGE.json"));
        let tmp_dir = std::env::temp_dir().join("betfair_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let tmp_file = tmp_dir.join("test_single_mcm.json");
        std::fs::write(&tmp_file, &data).unwrap();

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let instrument_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Instrument(_)))
            .count();
        assert!(
            instrument_count > 0,
            "should parse instruments from market definition"
        );
        assert_eq!(loader.instruments().len(), instrument_count);

        let has_sequence = items
            .iter()
            .any(|i| matches!(i, BetfairDataItem::SequenceCompleted(_)));
        assert!(has_sequence, "should emit SequenceCompleted");

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_mcm_with_book_data() {
        let sub_image = compact_json(&load_test_json("stream/mcm_SUB_IMAGE.json"));
        let update = compact_json(&load_test_json("stream/mcm_UPDATE.json"));

        let tmp_dir = std::env::temp_dir().join("betfair_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let tmp_file = tmp_dir.join("test_book_data.json");
        std::fs::write(&tmp_file, format!("{sub_image}\n{update}")).unwrap();

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let deltas_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Deltas(_)))
            .count();
        assert!(deltas_count > 0, "should parse book deltas");

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_instruments_only() {
        let sub_image = compact_json(&load_test_json("stream/mcm_SUB_IMAGE.json"));
        let update = compact_json(&load_test_json("stream/mcm_UPDATE.json"));

        let tmp_dir = std::env::temp_dir().join("betfair_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let tmp_file = tmp_dir.join("test_instruments_only.json");
        std::fs::write(&tmp_file, format!("{sub_image}\n{update}")).unwrap();

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let instruments = loader.load_instruments(&tmp_file).unwrap();

        assert!(!instruments.is_empty(), "should find instruments");
        assert_eq!(loader.instruments().len(), instruments.len());

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_reset_clears_state() {
        let data = compact_json(&load_test_json("stream/mcm_SUB_IMAGE.json"));
        let tmp_dir = std::env::temp_dir().join("betfair_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let tmp_file = tmp_dir.join("test_reset.json");
        std::fs::write(&tmp_file, &data).unwrap();

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        loader.load(&tmp_file).unwrap();
        assert!(!loader.instruments().is_empty());

        loader.reset();
        assert!(loader.instruments().is_empty());
        assert!(loader.traded_volumes.is_empty());

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_bsp_data() {
        let raw = load_test_json("stream/mcm_BSP.json");
        let messages: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
        let lines: Vec<String> = messages
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect();

        let tmp_dir = std::env::temp_dir().join("betfair_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let tmp_file = tmp_dir.join("test_bsp.json");
        std::fs::write(&tmp_file, lines.join("\n")).unwrap();

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let bsp_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::BspBookDelta(_)))
            .count();
        assert!(bsp_count > 0, "should parse BSP book deltas");

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_market_definition_with_traded_volumes() {
        let sub_image = compact_json(&load_test_json("stream/mcm_SUB_IMAGE.json"));
        let update_tv = compact_json(&load_test_json("stream/mcm_UPDATE_tv.json"));

        let tmp_dir = std::env::temp_dir().join("betfair_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let tmp_file = tmp_dir.join("test_tv.json");
        std::fs::write(&tmp_file, format!("{sub_image}\n{update_tv}")).unwrap();

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let ticker_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Ticker(_)))
            .count();
        assert!(ticker_count > 0, "should parse ticker data from tv updates");

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    #[ignore] // Requires user-fetched data in tests/test_data/local/betfair/
    fn test_load_match_odds_file() {
        let filepath = local_data_dir().join("1.253378068.gz");
        if !filepath.exists() {
            eprintln!("Skipping: {filepath:?} not found");
            return;
        }

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&filepath).unwrap();

        let instrument_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Instrument(_)))
            .count();
        let deltas_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Deltas(_)))
            .count();
        let trade_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Trade(_)))
            .count();
        let close_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::InstrumentClose(_)))
            .count();

        println!(
            "Match odds file: {instrument_count} instruments, {deltas_count} deltas, {trade_count} trades, {close_count} closes"
        );
        println!("Total items: {}", items.len());

        // 3 runners (home/draw/away), emitted on each market definition
        assert!(instrument_count >= 3, "expected at least 3 instruments");
        assert!(deltas_count > 0, "expected book deltas");
        assert!(trade_count > 0, "expected trade ticks");
        assert!(close_count > 0, "expected instrument closes at settlement");

        // Winner should be runner 2426
        let closes: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                BetfairDataItem::InstrumentClose(c) => Some(c),
                _ => None,
            })
            .collect();
        let winner = closes.iter().find(|c| c.close_price == Price::from("1.00"));
        assert!(winner.is_some(), "expected a winner with close_price 1.00");
        assert!(
            winner
                .unwrap()
                .instrument_id
                .symbol
                .as_str()
                .contains("2426"),
            "winner should be runner 2426"
        );
    }

    #[rstest]
    #[ignore] // Requires user-fetched data in tests/test_data/local/betfair/
    fn test_load_racing_win_file() {
        let filepath = local_data_dir().join("1.245077076.gz");
        if !filepath.exists() {
            eprintln!("Skipping: {filepath:?} not found");
            return;
        }

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&filepath).unwrap();

        let instrument_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Instrument(_)))
            .count();
        let deltas_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Deltas(_)))
            .count();
        let trade_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::Trade(_)))
            .count();
        let close_count = items
            .iter()
            .filter(|i| matches!(i, BetfairDataItem::InstrumentClose(_)))
            .count();

        println!(
            "Racing file: {instrument_count} instruments, {deltas_count} deltas, {trade_count} trades, {close_count} closes"
        );
        println!("Total items: {}", items.len());

        // 6 runners (though 2 removed during the race)
        assert!(instrument_count >= 6, "expected at least 6 instruments");
        assert!(deltas_count > 0, "expected book deltas");
        assert!(trade_count > 0, "expected trade ticks");
        assert!(close_count > 0, "expected instrument closes at settlement");

        // Winner should be runner 75925986
        let closes: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                BetfairDataItem::InstrumentClose(c) => Some(c),
                _ => None,
            })
            .collect();
        let winner = closes.iter().find(|c| c.close_price == Price::from("1.00"));
        assert!(winner.is_some(), "expected a winner with close_price 1.00");
        assert!(
            winner
                .unwrap()
                .instrument_id
                .symbol
                .as_str()
                .contains("75925986"),
            "winner should be runner 75925986"
        );
    }

    fn write_tmp(contents: &str, name: &str) -> PathBuf {
        let tmp_dir = std::env::temp_dir().join("betfair_test");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let tmp_file = tmp_dir.join(name);
        std::fs::write(&tmp_file, contents).unwrap();
        tmp_file
    }

    fn find_first(
        items: &[BetfairDataItem],
        pred: impl Fn(&BetfairDataItem) -> bool,
    ) -> Option<usize> {
        items.iter().position(pred)
    }

    fn find_last(
        items: &[BetfairDataItem],
        pred: impl Fn(&BetfairDataItem) -> bool,
    ) -> Option<usize> {
        items.iter().rposition(pred)
    }

    /// Split the loader output into per-MCM slices using `SequenceCompleted`
    /// as the delimiter. Each MCM ends with one `SequenceCompleted` item.
    fn partition_by_mcm(items: &[BetfairDataItem]) -> Vec<&[BetfairDataItem]> {
        let mut partitions = Vec::new();
        let mut start = 0;

        for (i, item) in items.iter().enumerate() {
            if matches!(item, BetfairDataItem::SequenceCompleted(_)) {
                partitions.push(&items[start..=i]);
                start = i + 1;
            }
        }

        partitions
    }

    #[rstest]
    fn test_load_emits_instrument_before_status_and_close() {
        // The loader must emit `Instrument` events before any `InstrumentStatus`
        // or `InstrumentClose` in the same MCM so downstream consumers (e.g. the
        // backtest exchange) have the instrument cached before lifecycle events
        // are processed.
        let data = compact_json(&load_test_json("stream/mcm_UPDATE_md.json"));
        let tmp_file = write_tmp(&data, "test_order_instrument_first.json");

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let instrument_idx =
            find_first(&items, |i| matches!(i, BetfairDataItem::Instrument(_))).unwrap();
        let status_idx = find_first(&items, |i| matches!(i, BetfairDataItem::Status(_))).unwrap();

        assert!(
            instrument_idx < status_idx,
            "Instrument (idx {instrument_idx}) must precede Status (idx {status_idx})"
        );

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_emits_instrument_before_close() {
        // Synthetic fixture: market CLOSED with terminal runner statuses so that
        // both Instrument and InstrumentClose are emitted within the same MCM.
        // Instrument must appear first.
        let mcm = r#"{"op":"mcm","id":1,"pt":1627617202953,"ct":"SUB_IMAGE","mc":[{"id":"1.1","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":false,"persistenceEnabled":false,"marketBaseRate":5,"eventId":"1","eventTypeId":"1","numberOfWinners":1,"bettingType":"ODDS","marketType":"WIN","marketTime":"2021-07-30T03:55:00.000Z","bspReconciled":true,"complete":true,"inPlay":false,"crossMatching":false,"runnersVoidable":false,"numberOfActiveRunners":0,"betDelay":0,"status":"CLOSED","runners":[{"status":"WINNER","sortPriority":1,"id":101},{"status":"LOSER","sortPriority":2,"id":102}],"regulators":["MR_INT"],"discountAllowed":true,"timezone":"UTC","openDate":"2021-07-30T02:45:00.000Z","version":1,"priceLadderDefinition":{"type":"CLASSIC"}}}]}"#;
        let tmp_file = write_tmp(mcm, "test_order_instrument_before_close.json");

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let instrument_idx =
            find_first(&items, |i| matches!(i, BetfairDataItem::Instrument(_))).unwrap();
        let close_idx =
            find_first(&items, |i| matches!(i, BetfairDataItem::InstrumentClose(_))).unwrap();

        assert!(
            instrument_idx < close_idx,
            "Instrument (idx {instrument_idx}) must precede InstrumentClose (idx {close_idx})"
        );

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_non_snapshot_deltas_tail_after_trades() {
        // Non-snapshot runner updates must emit book deltas AFTER any trades or
        // tickers parsed from the same message, matching the Python
        // `market_change_to_updates` ordering and keeping live/backtest in step.
        let data = compact_json(&load_test_json("stream/mcm_live_UPDATE.json"));
        let tmp_file = write_tmp(&data, "test_order_deltas_tail.json");

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        // Fixture is a single non-snapshot MCM (img=None) with trd + atl on one rc
        let last_trade_idx = find_last(&items, |i| matches!(i, BetfairDataItem::Trade(_))).unwrap();
        let first_deltas_idx =
            find_first(&items, |i| matches!(i, BetfairDataItem::Deltas(_))).unwrap();

        assert!(
            first_deltas_idx > last_trade_idx,
            "Deltas (first idx {first_deltas_idx}) must tail after Trade (last idx {last_trade_idx}) on non-snapshot updates"
        );

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_snapshot_deltas_emit_inline_before_trades() {
        // Snapshot messages (mc.img=true) emit Clear+Add deltas inline per runner
        // so consumers can apply the book state before any trades in the same
        // MCM. This matches Python's inline-snapshot behaviour.
        let data = compact_json(&load_test_json(
            "stream/market_definition_runner_removed.json",
        ));
        let tmp_file = write_tmp(&data, "test_order_snapshot_inline.json");

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let first_deltas_idx =
            find_first(&items, |i| matches!(i, BetfairDataItem::Deltas(_))).unwrap();
        let first_trade_idx =
            find_first(&items, |i| matches!(i, BetfairDataItem::Trade(_))).unwrap();

        assert!(
            first_deltas_idx < first_trade_idx,
            "Snapshot Deltas (first idx {first_deltas_idx}) must emit before Trade (first idx {first_trade_idx})"
        );

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_bsp_tails_after_book_deltas() {
        // Within each MCM, BSP deltas must emit after all regular book deltas.
        // Python flushes `book_updates` before `bsp_book_updates`; the Rust
        // loader must do the same to preserve consumer ordering.
        let raw = load_test_json("stream/mcm_BSP.json");
        let messages: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
        let lines: Vec<String> = messages
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect();
        let tmp_file = write_tmp(&lines.join("\n"), "test_order_bsp_tail.json");

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let partitions = partition_by_mcm(&items);
        assert!(
            !partitions.is_empty(),
            "expected at least one MCM partition"
        );

        let mut checked_any = false;

        for partition in partitions {
            let last_deltas_idx = find_last(partition, |i| matches!(i, BetfairDataItem::Deltas(_)));
            let first_bsp_idx =
                find_first(partition, |i| matches!(i, BetfairDataItem::BspBookDelta(_)));

            if let (Some(last_deltas), Some(first_bsp)) = (last_deltas_idx, first_bsp_idx) {
                assert!(
                    last_deltas < first_bsp,
                    "BspBookDelta (first idx {first_bsp}) must tail after Deltas (last idx {last_deltas}) within the same MCM"
                );
                checked_any = true;
            }
        }

        assert!(
            checked_any,
            "expected at least one MCM to contain both Deltas and BspBookDelta"
        );

        std::fs::remove_file(&tmp_file).ok();
    }

    #[rstest]
    fn test_load_emits_close_for_removed_runner_while_market_open() {
        // Removed runners must fire InstrumentClose as soon as the market
        // definition reports the Removed status, regardless of whether the
        // market as a whole is still Open. This matches the Python parser.
        let mcm = r#"{"op":"mcm","id":1,"pt":1627617202953,"ct":"SUB_IMAGE","mc":[{"id":"1.2","marketDefinition":{"bspMarket":false,"turnInPlayEnabled":false,"persistenceEnabled":false,"marketBaseRate":5,"eventId":"1","eventTypeId":"1","numberOfWinners":1,"bettingType":"ODDS","marketType":"WIN","marketTime":"2021-07-30T03:55:00.000Z","bspReconciled":false,"complete":true,"inPlay":false,"crossMatching":false,"runnersVoidable":false,"numberOfActiveRunners":1,"betDelay":0,"status":"OPEN","runners":[{"status":"ACTIVE","sortPriority":1,"id":201},{"status":"REMOVED","sortPriority":2,"id":202}],"regulators":["MR_INT"],"discountAllowed":true,"timezone":"UTC","openDate":"2021-07-30T02:45:00.000Z","version":1,"priceLadderDefinition":{"type":"CLASSIC"}}}]}"#;
        let tmp_file = write_tmp(mcm, "test_order_close_for_removed.json");

        let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
        let items = loader.load(&tmp_file).unwrap();

        let closes: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                BetfairDataItem::InstrumentClose(c) => Some(c),
                _ => None,
            })
            .collect();

        assert_eq!(
            closes.len(),
            1,
            "Removed runner must produce exactly one InstrumentClose while market is Open"
        );
        assert!(
            closes[0].instrument_id.symbol.as_str().contains("202"),
            "close must target the removed runner (selection id 202)"
        );

        std::fs::remove_file(&tmp_file).ok();
    }
}
