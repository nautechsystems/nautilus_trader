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

#![cfg(feature = "streaming")]

//! Integration tests for reproducible replay of captured prediction market archives.
//!
//! Each test writes a small deterministic archive into a temporary catalog, records a replay
//! manifest for it, and replays the archive from that manifest through a real [`BacktestNode`].

use std::{collections::BTreeMap, fmt::Debug, fs, path::PathBuf, str::FromStr};

use nautilus_backtest::{
    config::{BacktestRunConfig, BacktestVenueConfig, NautilusDataType},
    node::BacktestNode,
    replay::{ReplayDataset, ReplayLimitation, ReplayManifest, ReplaySource},
};
use nautilus_common::actor::DataActor;
use nautilus_core::UnixNanos;
use nautilus_execution::models::fee::{FeeModelAny, MakerTakerFeeModel};
use nautilus_model::{
    data::{Data, QuoteTick},
    enums::{AccountType, BookType, OmsType, OrderSide},
    identifiers::{InstrumentId, OutcomeGroupId, StrategyId, Symbol, Venue},
    instruments::{BinaryOption, InstrumentAny, stubs::binary_option},
    prediction::{
        Exclusivity, Exhaustiveness, MarketResolution, OutcomeGroup, OutcomeLeg, OutcomePayout,
        ResolutionOutcome, ResolutionSource,
    },
    types::{Money, Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore, nautilus_strategy};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::Value;
use tempfile::TempDir;
use ustr::Ustr;

const VENUE: &str = "POLYMARKET";
const RUN_ID: &str = "replay-test";
const STARTING_BALANCE: &str = "1_000.00 USDC";
const TRADE_SIZE: &str = "100.00";
const CONDITION: &str = "0xC0ND1T10N";
const CONDITION_YES: &str = "0xC0ND1T10N-YES";
const CONDITION_NO: &str = "0xC0ND1T10N-NO";
const OPEN_CONDITION: &str = "0xUNRES0LVED";
const OPEN_CONDITION_UP: &str = "0xUNRES0LVED-UP";

const START_NS: u64 = 1_704_067_200_000_000_000; // 2024-01-01T00:00:00Z
const END_NS: u64 = 1_704_153_600_000_000_000; // 2024-01-02T00:00:00Z
const RESOLUTION_NS: u64 = START_NS + 60_000_000_000;
const EXPIRATION_NS: u64 = END_NS + 86_400_000_000_000;

/// The USDC balance after buying 100.00 of each leg and settling the resolved one at 1.00:
/// 1,000.00 less 35.00 and 20.00 of purchases and 0.55 of commissions, plus a 100.00 payout.
const EXPECTED_BALANCE: &str = "1044.45000000 USDC";
/// The commissions of buying 100.00 at 0.350 plus 100.00 at 0.200 at a 1% taker rate.
const EXPECTED_FEES: &str = "0.55";
/// The resolved leg's realized result: +65.00 of payout move, net of its 0.35 commission.
const EXPECTED_REALIZED_PNL: &str = "64.65000000 USDC";

struct Archive {
    _temp: TempDir,
    manifest_path: PathBuf,
}

#[derive(Debug)]
struct ReplayOutcome {
    digest: String,
    summary: BTreeMap<String, String>,
    canonical: Value,
}

fn yes_leg() -> InstrumentId {
    InstrumentId::from(format!("{CONDITION_YES}.{VENUE}").as_str())
}

fn up_leg() -> InstrumentId {
    InstrumentId::from(format!("{OPEN_CONDITION_UP}.{VENUE}").as_str())
}

/// Writes the archive, then records its manifest.
///
/// `effective_ns` places the captured outcome in time; the record itself is always written inside
/// the window so that the archive holds it.
fn capture(captured_book: bool, effective_ns: u64) -> anyhow::Result<Archive> {
    let temp = TempDir::new()?;
    let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);

    catalog.write_instruments(vec![
        leg(CONDITION_YES),
        leg(CONDITION_NO),
        leg(OPEN_CONDITION_UP),
    ])?;
    let mut quote_count = 0;

    for (symbol, bid, ask) in [
        (CONDITION_YES, "0.340", "0.350"),
        (OPEN_CONDITION_UP, "0.190", "0.200"),
    ] {
        // The catalog stores one identity per file, so each leg is written on its own.
        let quotes = captured_quotes(symbol, bid, ask);
        quote_count += quotes.len();
        catalog.write_to_parquet(&quotes, None, None, None)?;
    }

    catalog.write_data_enum(
        &[Data::MarketResolution(resolution(
            effective_ns,
            RESOLUTION_NS,
        )?)],
        None,
        None,
        None,
    )?;

    let window = (UnixNanos::from(START_NS), UnixNanos::from(END_NS));
    let mut datasets = vec![
        ReplayDataset::capture(
            &catalog,
            NautilusDataType::QuoteTick,
            vec![yes_leg().to_string(), up_leg().to_string()],
            window.0,
            window.1,
            u64::try_from(quote_count)?,
            Vec::new(),
        )?,
        ReplayDataset::capture(
            &catalog,
            NautilusDataType::MarketResolution,
            Vec::new(),
            window.0,
            window.1,
            1,
            Vec::new(),
        )?,
    ];

    datasets.push(ReplayDataset::capture(
        &catalog,
        NautilusDataType::OrderBookDelta,
        Vec::new(),
        window.0,
        window.1,
        0,
        vec![if captured_book {
            ReplayLimitation::NotCaptured {
                reason: "no historical book".to_string(),
            }
        } else {
            ReplayLimitation::Gap {
                start: window.0,
                end: window.1,
            }
        }],
    )?);

    let manifest = ReplayManifest::new(
        ReplaySource {
            venue: VENUE.to_string(),
            loader: "replay_test_capture".to_string(),
            captured_at: UnixNanos::from(END_NS),
        },
        UnixNanos::from(START_NS),
        UnixNanos::from(END_NS),
        datasets,
    )?;
    manifest.verify_files()?;
    let manifest_path = temp.path().join("replay-manifest.json");
    manifest.write(&manifest_path)?;

    Ok(Archive {
        _temp: temp,
        manifest_path,
    })
}

fn replay_with(manifest: &ReplayManifest, run_id: &str) -> anyhow::Result<ReplayOutcome> {
    replay(manifest, run_id, BookType::L1_MBP)
}

fn replay(
    manifest: &ReplayManifest,
    run_id: &str,
    book_type: BookType,
) -> anyhow::Result<ReplayOutcome> {
    let venue_config = BacktestVenueConfig::builder()
        .name(Ustr::from(VENUE))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Cash)
        .book_type(book_type)
        .starting_balances(vec![STARTING_BALANCE.to_string()])
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel))
        .build()?;
    let run_config = BacktestRunConfig::builder()
        .id(run_id.to_string())
        .venues(vec![venue_config])
        .data(manifest.data_configs()?)
        .start(UnixNanos::from(START_NS))
        .end(UnixNanos::from(END_NS))
        // The assertions read the final state from the engine, so the run keeps it.
        .dispose_on_completion(false)
        // A replay must fail loudly rather than report a run that could not settle.
        .raise_exception(true)
        .build()?;
    let mut node = BacktestNode::new(vec![run_config])?;
    node.build()?;

    let engine = node
        .get_engine_mut(run_id)
        .ok_or_else(|| anyhow::anyhow!("no engine for run '{run_id}'"))?;
    for group in declared_groups()? {
        engine.add_outcome_group(group)?;
    }
    engine.add_strategy(BuyOnce::new(yes_leg(), up_leg()))?;

    let results = node.run()?;
    let result = results
        .first()
        .ok_or_else(|| anyhow::anyhow!("no result for run '{run_id}'"))?;
    let engine = node
        .get_engine(run_id)
        .ok_or_else(|| anyhow::anyhow!("no engine for run '{run_id}'"))?;
    let canonical = engine.get_canonical_result()?;

    Ok(ReplayOutcome {
        digest: canonical.digest()?,
        summary: result.summary.clone().into_iter().collect(),
        canonical: canonical.as_value().clone(),
    })
}

/// Returns the positions still held when the replay ended, which are its unsettled claims.
fn open_positions(canonical: &Value) -> Vec<&Value> {
    canonical["positions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|position| {
            position["quantity"]
                .as_str()
                .and_then(|quantity| Decimal::from_str(quantity).ok())
                .is_some_and(|quantity| quantity > Decimal::ZERO)
        })
        .collect()
}

/// Returns the instrument identities of the positions still open when the replay ended.
fn open_position_instruments(canonical: &Value) -> Vec<String> {
    open_positions(canonical)
        .iter()
        .filter_map(|position| position["instrument_id"].as_str().map(ToString::to_string))
        .collect()
}

/// Returns the position the run opened for `instrument_id`.
fn position_for<'a>(canonical: &'a Value, instrument_id: &InstrumentId) -> Option<&'a Value> {
    canonical["positions"]
        .as_array()?
        .iter()
        .find(|position| position["instrument_id"].as_str() == Some(&instrument_id.to_string()))
}

/// Decodes a canonical `f64`, which the canonical document stores as its hexadecimal bit pattern.
fn decode_canonical_f64(value: &str) -> Option<f64> {
    u64::from_str_radix(value, 16).ok().map(f64::from_bits)
}

/// Returns the summed commissions the run's fills declared, per currency.
fn fill_fees(canonical: &Value) -> BTreeMap<String, Decimal> {
    let mut fees: BTreeMap<String, Decimal> = BTreeMap::new();

    for fill in canonical["fills"].as_array().into_iter().flatten() {
        let Some(commission) = fill["event"]["Filled"]["commission"].as_str() else {
            continue;
        };
        let Ok(money) = Money::from_str(commission) else {
            continue;
        };
        *fees.entry(money.currency.code.to_string()).or_default() += money.as_decimal();
    }

    fees
}

fn balance<'a>(summary: &'a BTreeMap<String, String>, currency: &str) -> Option<&'a str> {
    summary
        .get(&format!("account.{VENUE}.balance.{currency}.total"))
        .map(String::as_str)
}

#[rstest]
fn test_offline_replay_settles_the_resolution_and_reports_the_exact_final_state() {
    let archive = capture(false, RESOLUTION_NS).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();
    let outcome = replay_with(&manifest, RUN_ID).unwrap();

    // Final cash: the 1,000.00 start, less both purchases and their commissions, plus the payout.
    assert_eq!(balance(&outcome.summary, "USDC"), Some(EXPECTED_BALANCE));
    assert_eq!(
        fill_fees(&outcome.canonical)["USDC"],
        Decimal::from_str(EXPECTED_FEES).unwrap()
    );

    // The resolved leg settled at its declared payout and is no longer an open position.
    let open = open_position_instruments(&outcome.canonical);

    assert!(
        !open.contains(&yes_leg().to_string()),
        "the resolved leg must not remain open: {open:?}"
    );
    let settled = position_for(&outcome.canonical, &yes_leg()).unwrap();

    assert_eq!(settled["quantity"], "0.00");
    assert_eq!(settled["realized_pnl"], EXPECTED_REALIZED_PNL);

    // The capture declares a gap in the book, so the replay reports its data as incomplete.
    assert!(!manifest.is_complete());
}

#[rstest]
fn test_offline_replay_reports_an_unsettled_claim_for_an_unresolved_condition() {
    let archive = capture(false, RESOLUTION_NS).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();
    let outcome = replay_with(&manifest, RUN_ID).unwrap();

    // The venue never resolved this condition inside the archive, so the position stays a claim.
    assert_eq!(
        open_position_instruments(&outcome.canonical),
        vec![up_leg().to_string()]
    );

    let claim = position_for(&outcome.canonical, &up_leg()).unwrap();

    assert_eq!(claim["side"], "LONG");
    assert_eq!(claim["quantity"], "100.00");
    // The claim carries the commission its entry paid, and nothing realized yet.
    assert_eq!(claim["realized_pnl"], "-0.20000000 USDC");
    assert_eq!(
        claim["avg_px_open"].as_str().and_then(decode_canonical_f64),
        Some(0.2)
    );
}

#[rstest]
fn test_repeated_replay_of_the_same_manifest_is_deterministic() {
    let archive = capture(false, RESOLUTION_NS).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();

    let first = replay_with(&manifest, RUN_ID).unwrap();
    let second = replay_with(&manifest, RUN_ID).unwrap();

    assert_eq!(first.digest, second.digest);
    assert_eq!(first.canonical, second.canonical);
}

#[rstest]
fn test_replay_keeps_the_terminal_outcome_out_of_pre_resolution_fills() {
    let archive = capture(false, RESOLUTION_NS).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();
    let outcome = replay_with(&manifest, RUN_ID).unwrap();

    // The entry filled at the historical quote and the position closed at the payout the
    // resolution declared. No other price appears, so the terminal outcome was never applied to a
    // pre-resolution fill.
    let mut fills = outcome.canonical["fills"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fill| fill["event"]["Filled"]["instrument_id"] == yes_leg().to_string())
        .filter_map(|fill| fill["event"]["Filled"]["last_px"].as_str())
        .collect::<Vec<_>>();
    fills.sort_unstable();

    assert_eq!(fills, vec!["0.350", "1.000"]);

    let settled = position_for(&outcome.canonical, &yes_leg()).unwrap();

    assert_eq!(
        settled["avg_px_open"]
            .as_str()
            .and_then(decode_canonical_f64),
        Some(0.35)
    );
    assert_eq!(
        settled["avg_px_close"]
            .as_str()
            .and_then(decode_canonical_f64),
        Some(1.0)
    );
}

#[rstest]
fn test_replay_rejects_an_absent_resolution_timestamp() {
    let archive = capture(false, 0).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();

    let error = replay_with(&manifest, RUN_ID).unwrap_err();

    assert!(
        error.to_string().contains("no effective timestamp"),
        "{error}"
    );
}

#[rstest]
fn test_replay_rejects_a_venue_that_needs_a_book_the_archive_lacks() {
    let archive = capture(true, RESOLUTION_NS).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();

    // The archive declares no historical book, so an L2 venue cannot replay from it.
    let error = replay(&manifest, RUN_ID, BookType::L2_MBP).unwrap_err();

    assert!(
        error.to_string().contains("no order book data configured"),
        "{error}"
    );
}

#[rstest]
fn test_replay_rejects_an_archive_whose_file_no_longer_matches() {
    let archive = capture(false, RESOLUTION_NS).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();
    let dataset = &manifest.datasets[0];
    // The capture writes its manifest inside the catalog root it recorded.
    let catalog_root = archive.manifest_path.parent().unwrap();

    // Truncating a captured file must fail verification rather than replay a shorter history.
    fs::write(catalog_root.join(&dataset.files[0]), b"").unwrap();

    let error = manifest.verify_files().unwrap_err();

    assert!(error.to_string().contains("checksum mismatch"), "{error}");
}

#[rstest]
fn test_manifest_declaring_an_uncaptured_book_is_not_queried() {
    let archive = capture(true, RESOLUTION_NS).unwrap();
    let manifest = ReplayManifest::load(&archive.manifest_path).unwrap();

    assert_eq!(manifest.data_configs().unwrap().len(), 2);
    assert_eq!(manifest.limitations().len(), 1);
    assert_eq!(
        manifest.limitations()[0].1.describe(),
        "not captured: no historical book"
    );
}

/// Builds one captured binary option leg priced at a 0.001 increment.
fn leg(symbol: &str) -> InstrumentAny {
    let mut instrument: BinaryOption = binary_option();
    instrument.id = InstrumentId::from(format!("{symbol}.{VENUE}").as_str());
    instrument.raw_symbol = Symbol::from(symbol);
    instrument.activation_ns = UnixNanos::from(START_NS);
    instrument.expiration_ns = UnixNanos::from(EXPIRATION_NS);
    instrument.taker_fee = Decimal::new(1, 2);
    InstrumentAny::BinaryOption(instrument)
}

/// Returns the deterministic quote history the capture stores for one leg: a 0.350 ask on the
/// resolved leg and a 0.200 ask on the condition the venue never resolved.
fn captured_quotes(symbol: &str, bid: &str, ask: &str) -> Vec<QuoteTick> {
    let mut quotes = Vec::new();

    for tick in 0..5u64 {
        let ts = UnixNanos::from(START_NS + tick * 1_000_000_000);
        quotes.push(QuoteTick::new(
            InstrumentId::from(format!("{symbol}.{VENUE}").as_str()),
            Price::from(bid),
            Price::from(ask),
            Quantity::from("1000.00"),
            Quantity::from("1000.00"),
            ts,
            ts,
        ));
    }

    quotes
}

fn declared_groups() -> anyhow::Result<Vec<OutcomeGroup>> {
    let venue = Venue::from(VENUE);
    let unit_total = Money::from("1.00 USDC");

    Ok(vec![
        OutcomeGroup::new_checked(
            OutcomeGroupId::from_parts(venue, CONDITION)?,
            None,
            vec![
                OutcomeLeg::new(Ustr::from("Yes"), yes_leg(), unit_total),
                OutcomeLeg::new(
                    Ustr::from("No"),
                    InstrumentId::from(format!("{CONDITION_NO}.{VENUE}").as_str()),
                    Money::from("0.00 USDC"),
                ),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            unit_total,
            1,
            None,
            UnixNanos::from(START_NS),
            UnixNanos::from(START_NS),
        )?,
        OutcomeGroup::new_checked(
            OutcomeGroupId::from_parts(venue, OPEN_CONDITION)?,
            None,
            vec![OutcomeLeg::new(Ustr::from("Up"), up_leg(), unit_total)],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::from(START_NS),
            UnixNanos::from(START_NS),
        )?,
    ])
}

/// Returns a captured resolution that took effect at `effective_ns` and was recorded at
/// `recorded_ns`, which is what places it inside the captured window.
fn resolution(effective_ns: u64, recorded_ns: u64) -> anyhow::Result<MarketResolution> {
    Ok(MarketResolution {
        group_id: OutcomeGroupId::from_parts(Venue::from(VENUE), CONDITION)?,
        version: 1,
        source: ResolutionSource::new(Venue::from(VENUE), "uma-request-1", None),
        outcome: ResolutionOutcome::Payouts(vec![
            OutcomePayout::new(Ustr::from("Yes"), Money::from("1.00 USDC")),
            OutcomePayout::new(Ustr::from("No"), Money::from("0.00 USDC")),
        ]),
        effective_ns: UnixNanos::from(effective_ns),
        observed_ns: UnixNanos::from(recorded_ns),
        ts_event: UnixNanos::from(recorded_ns),
        ts_init: UnixNanos::from(recorded_ns),
    })
}

/// Buys a fixed size of each captured leg once, then holds to resolution.
struct BuyOnce {
    core: StrategyCore,
    resolved_leg: InstrumentId,
    open_leg: InstrumentId,
    trade_size: Quantity,
    submitted: usize,
}

impl BuyOnce {
    fn new(resolved_leg: InstrumentId, open_leg: InstrumentId) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("REPLAY-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            resolved_leg,
            open_leg,
            trade_size: Quantity::from(TRADE_SIZE),
            submitted: 0,
        }
    }
}

nautilus_strategy!(BuyOnce);

impl Debug for BuyOnce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BuyOnce)).finish()
    }
}

impl DataActor for BuyOnce {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.resolved_leg, None, None);
        self.subscribe_quotes(self.open_leg, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        let instrument_id = quote.instrument_id;

        if (instrument_id == self.resolved_leg && self.submitted == 0)
            || (instrument_id == self.open_leg && self.submitted == 1)
        {
            self.submitted += 1;
            let order = self.order().market(
                instrument_id,
                OrderSide::Buy,
                self.trade_size,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            );
            self.submit_order(order, None, None, None)?;
        }

        Ok(())
    }
}
