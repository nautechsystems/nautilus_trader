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

//! Example: one offline command that replays a prediction market outcome group.
//!
//! The command captures a deterministic archive into a temporary catalog (instruments, quotes, and
//! one authoritative resolution), records a [`ReplayManifest`] for it, and then replays the archive
//! twice from that manifest alone. It reports the final cash balances, positions, fees, and
//! unsettled claims, and proves the two replays are identical by comparing canonical result
//! digests.
//!
//! The archive states its own limits: no historical order book is captured for this venue, so the
//! replay is a trades-and-resolutions replay and reports that scope instead of implying depth it
//! does not have.
//!
//! Run with: `cargo run -p nautilus-backtest --features examples,streaming --example prediction-market-replay`

#[cfg(feature = "mimalloc")]
mod allocator;

use std::{collections::BTreeMap, fmt::Debug, fs, path::Path, str::FromStr};

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
use rust_decimal::Decimal;
use serde_json::Value;
use tempfile::TempDir;
use ustr::Ustr;

const VENUE: &str = "POLYMARKET";
const RUN_ID: &str = "prediction-replay";
const STARTING_BALANCE: &str = "1_000.00 USDC";
const TRADE_SIZE: &str = "100.00";

/// The resolved condition and its two legs.
const CONDITION: &str = "0xC0ND1T10N";
const CONDITION_YES: &str = "0xC0ND1T10N-YES";
const CONDITION_NO: &str = "0xC0ND1T10N-NO";
/// A second condition the venue declared but never resolved inside this archive.
const OPEN_CONDITION: &str = "0xUNRES0LVED";
const OPEN_CONDITION_UP: &str = "0xUNRES0LVED-UP";

/// The capture window, expressed in UNIX nanoseconds.
const START_NS: u64 = 1_704_067_200_000_000_000; // 2024-01-01T00:00:00Z
const END_NS: u64 = 1_704_153_600_000_000_000; // 2024-01-02T00:00:00Z
/// The instant the venue's outcome took effect, one minute into the window.
const RESOLUTION_NS: u64 = START_NS + 60_000_000_000;
/// The venue's scheduled close, placed after the replay window so only the resolution settles.
const EXPIRATION_NS: u64 = END_NS + 86_400_000_000_000;

/// The replay command's report for one run.
#[derive(Debug)]
struct ReplayReport {
    digest: String,
    summary: BTreeMap<String, String>,
    fees: BTreeMap<String, Decimal>,
    settled: Vec<String>,
    unsettled: Vec<String>,
    canonical: Value,
}

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "mimalloc")]
    allocator::register();

    let temp = TempDir::new()?;
    let catalog_path = temp.path().to_str().unwrap().to_string();
    let manifest_path = temp.path().join("replay-manifest.json");

    // Capture: in production this step belongs to the loader, which writes the catalog and records
    // what it could and could not obtain.
    capture(&catalog_path, &manifest_path)?;

    // Replay: from here on the command knows only the manifest and the captured files.
    let manifest = ReplayManifest::load(&manifest_path)?;
    manifest.verify_files()?;
    report_scope(&manifest);

    let groups = declared_groups()?;
    let first = replay(&manifest, &groups, RUN_ID)?;
    let second = replay(&manifest, &groups, RUN_ID)?;
    report_run(&first);

    anyhow::ensure!(
        first.digest == second.digest,
        "repeated replay is not deterministic: {} != {}",
        first.digest,
        second.digest
    );
    println!("\nDeterminism: both replays produced {}", first.digest);

    let canonical = temp.path().join("canonical-result.json");
    fs::write(&canonical, serde_json::to_vec_pretty(&first.canonical)?)?;
    println!("Canonical result: {}", canonical.display());

    Ok(())
}

/// Returns the resolved condition's tradable leg identity.
fn yes_leg() -> InstrumentId {
    InstrumentId::from(format!("{CONDITION_YES}.{VENUE}").as_str())
}

/// Returns the identity of the leg the venue never resolved inside the archive.
fn up_leg() -> InstrumentId {
    InstrumentId::from(format!("{OPEN_CONDITION_UP}.{VENUE}").as_str())
}

/// Writes the captured archive, then records and verifies its manifest.
fn capture(catalog_path: &str, manifest_path: &Path) -> anyhow::Result<()> {
    let catalog = ParquetDataCatalog::new(Path::new(catalog_path), None, None, None, None);
    let instruments = vec![
        leg(CONDITION_YES, "0.010"),
        leg(CONDITION_NO, "0.010"),
        leg(OPEN_CONDITION_UP, "0.010"),
    ];
    catalog.write_instruments(instruments)?;

    let mut quote_count = 0u64;

    for (symbol, bid, ask) in [
        (CONDITION_YES, "0.340", "0.350"),
        (OPEN_CONDITION_UP, "0.190", "0.200"),
    ] {
        // The catalog stores one identity per file, so each leg is written on its own.
        let quotes = captured_quotes(symbol, bid, ask);
        quote_count += u64::try_from(quotes.len())?;
        catalog.write_to_parquet(&quotes, None, None, None)?;
    }

    catalog.write_data_enum(&[Data::MarketResolution(resolution()?)], None, None, None)?;

    let window = (UnixNanos::from(START_NS), UnixNanos::from(END_NS));
    let mut datasets = vec![
        ReplayDataset::capture(
            &catalog,
            NautilusDataType::QuoteTick,
            vec![yes_leg().to_string(), up_leg().to_string()],
            window.0,
            window.1,
            quote_count,
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
    anyhow::ensure!(
        datasets.iter().all(|dataset| !dataset.files.is_empty()),
        "the capture wrote no catalog files, so the manifest cannot describe it"
    );

    // The venue publishes no historical book for this window. Recording that limit is what keeps
    // the replay honest: depth is never inferred from trades.
    datasets.push(ReplayDataset::capture(
        &catalog,
        NautilusDataType::OrderBookDelta,
        Vec::new(),
        window.0,
        window.1,
        0,
        vec![ReplayLimitation::NotCaptured {
            reason: "the venue publishes no historical order book for this window".to_string(),
        }],
    )?);

    let manifest = ReplayManifest::new(
        ReplaySource {
            venue: VENUE.to_string(),
            loader: "example_capture".to_string(),
            captured_at: UnixNanos::from(END_NS + 3_600_000_000_000),
        },
        UnixNanos::from(START_NS),
        UnixNanos::from(END_NS),
        datasets,
    )?;
    manifest.verify_files()?;
    manifest.write(manifest_path)?;

    println!("Captured {quote_count} quotes and 1 resolution into {catalog_path}");

    Ok(())
}

/// Runs one replay of the archive through [`BacktestNode`].
fn replay(
    manifest: &ReplayManifest,
    groups: &[OutcomeGroup],
    run_id: &str,
) -> anyhow::Result<ReplayReport> {
    let venue_config = BacktestVenueConfig::builder()
        .name(Ustr::from(VENUE))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Cash)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![STARTING_BALANCE.to_string()])
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel))
        .build()?;
    let run_config = BacktestRunConfig::builder()
        .id(run_id.to_string())
        .venues(vec![venue_config])
        .data(manifest.data_configs()?)
        .start(UnixNanos::from(START_NS))
        .end(UnixNanos::from(END_NS))
        // The command reports the final state from the engine, so the run keeps it.
        .dispose_on_completion(false)
        // A replay must fail loudly rather than report a run that could not settle.
        .raise_exception(true)
        .build()?;
    let mut node = BacktestNode::new(vec![run_config])?;
    node.build()?;

    let engine = node
        .get_engine_mut(run_id)
        .ok_or_else(|| anyhow::anyhow!("no engine was built for run '{run_id}'"))?;
    // The group a venue declared must be in the cache before its resolution data replays.
    for group in groups {
        engine.add_outcome_group(group.clone())?;
    }
    engine.add_strategy(BuyOnce::new(
        InstrumentId::from(format!("{CONDITION_YES}.{VENUE}").as_str()),
        InstrumentId::from(format!("{OPEN_CONDITION_UP}.{VENUE}").as_str()),
    ))?;

    let results = node.run()?;
    let result = results
        .first()
        .ok_or_else(|| anyhow::anyhow!("the replay produced no result"))?;
    let engine = node
        .get_engine(run_id)
        .ok_or_else(|| anyhow::anyhow!("no engine was built for run '{run_id}'"))?;
    let canonical = engine.get_canonical_result()?;
    let value = canonical.as_value().clone();
    let mut summary: BTreeMap<String, String> = result.summary.clone().into_iter().collect();
    summary.retain(|key, _| key.starts_with("account.") || key.starts_with("positions."));

    Ok(ReplayReport {
        digest: canonical.digest()?,
        summary,
        fees: fill_fees(&value),
        settled: settled_positions(&value),
        unsettled: open_positions(&value),
        canonical: value,
    })
}

/// Returns the fee totals declared by the run's fills, per currency.
fn fill_fees(canonical: &Value) -> BTreeMap<String, Decimal> {
    let mut fees: BTreeMap<String, Decimal> = BTreeMap::new();

    for fill in canonical["fills"].as_array().into_iter().flatten() {
        let Some(commission) = fill["event"]["Filled"]["commission"].as_str() else {
            continue;
        };
        let Ok(money) = Money::from_str(commission) else {
            continue;
        };
        let total = fees.entry(money.currency.code.to_string()).or_default();
        *total += money.as_decimal();
    }

    fees
}

/// Returns the positions still held when the replay ended, which are its unsettled claims.
///
/// A fully settled leg keeps a zero-quantity position in the cache, so only non-zero quantities are
/// still claims on the venue.
fn open_positions(canonical: &Value) -> Vec<String> {
    canonical["positions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|position| {
            let quantity = position["quantity"].as_str()?;
            if Decimal::from_str(quantity).ok()? <= Decimal::ZERO {
                return None;
            }
            let price = position["avg_px_open"]
                .as_str()
                .and_then(decode_canonical_f64)
                .map_or_else(|| "<unknown>".to_string(), |px| format!("{px:.8}"));

            Some(format!(
                "{quantity} of {} opened at {price}",
                position["instrument_id"].as_str().unwrap_or("<unknown>"),
            ))
        })
        .collect()
}

/// Returns the positions the replay settled, with the result each one recorded.
fn settled_positions(canonical: &Value) -> Vec<String> {
    canonical["positions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|position| {
            let quantity = position["quantity"].as_str()?;
            if Decimal::from_str(quantity).ok()? > Decimal::ZERO {
                return None;
            }

            Some(format!(
                "{} realized {}",
                position["instrument_id"].as_str().unwrap_or("<unknown>"),
                position["realized_pnl"].as_str().unwrap_or("none"),
            ))
        })
        .collect()
}

/// Decodes a canonical `f64`, which the canonical document stores as its hexadecimal bit pattern.
fn decode_canonical_f64(value: &str) -> Option<f64> {
    u64::from_str_radix(value, 16).ok().map(f64::from_bits)
}

fn report_scope(manifest: &ReplayManifest) {
    println!("\nReplay manifest: {} datasets", manifest.datasets.len());

    for dataset in &manifest.datasets {
        println!(
            "  {} ({} identifiers, {} captured files)",
            dataset.data_type,
            dataset.identifiers.len(),
            dataset.files.len()
        );
    }

    if manifest.is_complete() {
        println!("Scope: complete history for the declared window");
    } else {
        println!("Scope: incomplete data, the replay reports only what was captured");
        for (dataset, limitation) in manifest.limitations() {
            println!("  {} {}", dataset.data_type, limitation.describe());
        }
    }
}

fn report_run(report: &ReplayReport) {
    println!("\nFinal state");
    for (key, value) in &report.summary {
        println!("  {key}: {value}");
    }

    if report.fees.is_empty() {
        println!("  fees: none");
    } else {
        for (currency, fee) in &report.fees {
            println!("  fees.{currency}: {fee}");
        }
    }

    if report.settled.is_empty() {
        println!("  settled positions: none");
    } else {
        println!("  settled positions:");
        for settled in &report.settled {
            println!("    {settled}");
        }
    }

    if report.unsettled.is_empty() {
        println!("  unsettled claims: none");
    } else {
        println!("  unsettled claims:");
        for claim in &report.unsettled {
            println!("    {claim}");
        }
    }
}

/// Returns the outcome groups the venue declared for the captured conditions.
fn declared_groups() -> anyhow::Result<Vec<OutcomeGroup>> {
    let venue = Venue::from(VENUE);
    let unit_total = Money::from("1.00 USDC");

    Ok(vec![
        OutcomeGroup::new_checked(
            OutcomeGroupId::from_parts(venue, CONDITION)?,
            Some(CONDITION.to_string()),
            vec![
                OutcomeLeg::new(
                    Ustr::from("Yes"),
                    InstrumentId::from(format!("{CONDITION_YES}.{VENUE}").as_str()),
                    unit_total,
                ),
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
            Some(Ustr::from("example_capture")),
            UnixNanos::from(START_NS),
            UnixNanos::from(START_NS),
        )?,
        OutcomeGroup::new_checked(
            OutcomeGroupId::from_parts(venue, OPEN_CONDITION)?,
            Some(OPEN_CONDITION.to_string()),
            vec![OutcomeLeg::new(
                Ustr::from("Up"),
                InstrumentId::from(format!("{OPEN_CONDITION_UP}.{VENUE}").as_str()),
                unit_total,
            )],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            unit_total,
            1,
            Some(Ustr::from("example_capture")),
            UnixNanos::from(START_NS),
            UnixNanos::from(START_NS),
        )?,
    ])
}

/// Returns the authoritative outcome the venue published for the captured condition.
fn resolution() -> anyhow::Result<MarketResolution> {
    Ok(MarketResolution {
        group_id: OutcomeGroupId::from_parts(Venue::from(VENUE), CONDITION)?,
        version: 1,
        source: ResolutionSource::new(Venue::from(VENUE), "uma-request-1", None),
        outcome: ResolutionOutcome::Payouts(vec![
            OutcomePayout::new(Ustr::from("Yes"), Money::from("1.00 USDC")),
            OutcomePayout::new(Ustr::from("No"), Money::from("0.00 USDC")),
        ]),
        effective_ns: UnixNanos::from(RESOLUTION_NS),
        observed_ns: UnixNanos::from(RESOLUTION_NS + 1_000_000_000),
        ts_event: UnixNanos::from(RESOLUTION_NS),
        ts_init: UnixNanos::from(RESOLUTION_NS + 1_000_000_000),
    })
}

/// Builds one captured binary option leg.
fn leg(symbol: &str, taker_fee: &str) -> InstrumentAny {
    let mut instrument: BinaryOption = binary_option();
    instrument.id = InstrumentId::from(format!("{symbol}.{VENUE}").as_str());
    instrument.raw_symbol = Symbol::from(symbol);
    instrument.activation_ns = UnixNanos::from(START_NS);
    instrument.expiration_ns = UnixNanos::from(EXPIRATION_NS);
    instrument.taker_fee = Decimal::from_str(taker_fee).unwrap();
    InstrumentAny::BinaryOption(instrument)
}

/// Returns the deterministic quote history the capture stores for one leg.
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
            strategy_id: Some(StrategyId::from("PREDICTION-001")),
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
