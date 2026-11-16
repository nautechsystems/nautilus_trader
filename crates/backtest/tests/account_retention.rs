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

use std::{collections::BTreeMap, fmt::Debug, time::Instant};

use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
};
use nautilus_common::{actor::DataActor, logging::logger::LoggerConfig};
use nautilus_core::UnixNanos;
use nautilus_execution::{
    engine::config::ExecutionEngineConfig,
    models::fee::{FeeModelAny, MakerTakerFeeModel},
};
use nautilus_model::{
    accounts::{Account, AccountAny},
    data::{Data, QuoteTick},
    enums::{AccountType, BookType, OmsType, OrderSide, OrderStatus},
    events::{AccountState, OrderEventAny, PortfolioSnapshot},
    identifiers::{InstrumentId, StrategyId},
    instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::Order,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore, nautilus_strategy};
use rstest::rstest;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use rust_decimal_macros::dec;
use serde_json::{Value, json};

const START_NS: u64 = 1_735_689_600_000_000_000;
const SECOND_NS: u64 = 1_000_000_000;
const DAY_NS: u64 = 86_400 * SECOND_NS;

#[rstest]
#[case::single_currency(false, true)]
#[case::multi_currency_fallback(true, true)]
#[case::position_returns(false, false)]
fn test_account_retention_preserves_financial_results(
    #[case] multi_currency: bool,
    #[case] equity_curve: bool,
) {
    let mut full = retention_engine(3, 2, None, multi_currency, equity_curve);
    full.run(None, None, None, false).unwrap();
    let expected = financial_result(&full, 3, 2, multi_currency, equity_curve);
    let full_events = account(&full).events();
    assert_eq!(full_events.len(), 9);

    for lookback in [0, 60, 1440, 10_000] {
        let mut purged = retention_engine(3, 2, Some(lookback), multi_currency, equity_curve);
        purged.run(None, None, None, false).unwrap();
        let actual = financial_result(&purged, 3, 2, multi_currency, equity_curve);
        let events = account(&purged).events();

        assert_eq!(actual, expected);

        let retained = match lookback {
            10_000 => 9,
            1440 => 4,
            _ => 1,
        };

        assert_eq!(events.len(), retained);
        assert_eq!(
            report_events(&events),
            report_events(&full_events[full_events.len() - retained..])
        );
        purged.dispose();
    }

    full.dispose();
}

#[rstest]
#[case::full(None)]
#[case::purged(Some(0))]
fn test_account_retention_preserves_overnight_equity(#[case] lookback: Option<u32>) {
    let instrument_id = crypto_perpetual_ethusdt().id;
    let mut engine = retention_venue(lookback, false, true);
    engine
        .add_strategy(RetentionOrders::new(instrument_id, 1, 86_399))
        .unwrap();
    let data = vec![
        retention_quote(instrument_id, 0, 0, "100.00"),
        retention_quote(instrument_id, 0, 86_399, "100.00"),
        retention_quote(instrument_id, 1, 3600, "102.00"),
    ];
    engine.add_data(data, None, true, true).unwrap();
    engine.run(None, None, None, false).unwrap();
    let account = account(&engine);
    let portfolio = engine.kernel().portfolio();
    let snapshots = portfolio.snapshots(&account.id());
    let equities = snapshots
        .iter()
        .map(|snapshot| (snapshot.ts_event, snapshot.total_equity[0].as_decimal()))
        .collect::<Vec<_>>();
    assert_eq!(
        equities,
        vec![
            (UnixNanos::new(START_NS), dec!(1_000_000)),
            (UnixNanos::new(START_NS + DAY_NS), dec!(999_999.9)),
            (
                UnixNanos::new(START_NS + DAY_NS + 3600 * SECOND_NS),
                dec!(1_000_001.9)
            ),
        ]
    );
    assert_eq!(snapshots[2].unrealized_pnls, vec![Money::from("2 USDT")]);
    assert_eq!(
        account
            .balance_total(Some(Currency::USDT()))
            .unwrap()
            .as_decimal(),
        dec!(999_999.9)
    );
    assert_eq!(
        account.event_count(),
        if lookback.is_some() { 1 } else { 2 }
    );
    let expected_returns = BTreeMap::from([
        (UnixNanos::new(START_NS), 999_999.9 / 1_000_000.0 - 1.0),
        (
            UnixNanos::new(START_NS + DAY_NS),
            1_000_001.9 / 999_999.9 - 1.0,
        ),
    ]);
    assert_eq!(portfolio.analyzer().portfolio_returns(), &expected_returns);
    drop(portfolio);
    engine.dispose();
}

#[rstest]
#[ignore = "runs one isolated retention measurement; see benches/BENCHMARKS.md"]
fn measure_account_retention() {
    let days: usize = std::env::var("RETENTION_DAYS").unwrap().parse().unwrap();
    let pairs: usize = std::env::var("RETENTION_PAIRS").unwrap().parse().unwrap();
    let lookback = std::env::var("RETENTION_LOOKBACK_MINS")
        .ok()
        .map(|value| value.parse().unwrap());
    let mut engine = retention_engine(days, pairs, lookback, false, true);
    let started = Instant::now();
    engine.run(None, None, None, false).unwrap();
    let elapsed = started.elapsed();

    let peak_rss_kib = std::env::var_os("RETENTION_MEMORY").map(|_| {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        status
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmHWM:").map(|value| {
                    value
                        .split_whitespace()
                        .next()
                        .unwrap()
                        .parse::<u64>()
                        .unwrap()
                })
            })
            .unwrap()
    });

    let retained_events = account(&engine).event_count();
    let result = financial_result(&engine, days, pairs, false, true);
    let fingerprint = blake3::hash(&serde_json::to_vec(&result).unwrap())
        .to_hex()
        .to_string();
    println!(
        "{}",
        json!({
            "days": days,
            "pairs_per_active_day": pairs,
            "lookback_mins": lookback,
            "run_elapsed_secs": peak_rss_kib.is_none().then_some(elapsed.as_secs_f64()),
            "peak_rss_kib": peak_rss_kib,
            "retained_events": retained_events,
            "fingerprint": fingerprint,
            "balance": result["balance"],
            "commission": result["commission"],
            "pnl": result["pnl"],
            "orders": result["orders"],
        })
    );
    engine.dispose();
}

fn financial_result(
    engine: &BacktestEngine,
    days: usize,
    pairs: usize,
    multi_currency: bool,
    equity_curve: bool,
) -> Value {
    let account = account(engine);
    let active_days = (0..days).filter(|day| day % 3 != 1).count();
    let completed_pairs = active_days * pairs;
    let net_pnl = dec!(1.798) * Decimal::from(completed_pairs);
    let balance = dec!(1_000_000) + net_pnl;
    assert_balances(&account, balance, multi_currency);
    let commission = assert_fills(engine, completed_pairs);

    let AccountAny::Margin(margin) = &account else {
        panic!("retention fixture must use a margin account");
    };

    let commissions = margin.base.commissions();
    assert_eq!(commissions.len(), 1);
    assert_eq!(commissions[&Currency::USDT()].as_decimal(), commission);

    let portfolio = engine.kernel().portfolio();
    let analyzer = portfolio.analyzer();
    let snapshots = portfolio.snapshots(&account.id());
    let expected_returns = assert_snapshots(&snapshots, days, pairs, equity_curve);

    if equity_curve && !multi_currency {
        assert_eq!(analyzer.portfolio_returns(), &expected_returns);
        assert_eq!(analyzer.returns(), &expected_returns);
    } else {
        assert!(analyzer.portfolio_returns().is_empty());
        assert_eq!(analyzer.returns(), analyzer.position_returns());
        assert_eq!(analyzer.position_returns().len(), completed_pairs);
        assert!(
            analyzer
                .position_returns()
                .values()
                .all(|value| *value == 0.02)
        );
    }

    let result = engine.get_result();

    let normalize_stats = |stats: &ahash::AHashMap<String, f64>| -> BTreeMap<String, String> {
        stats
            .iter()
            .map(|(name, value)| (name.clone(), value.to_string()))
            .collect()
    };

    json!({
        "balance": balance.to_string(),
        "commission": commission.to_string(),
        "pnl": net_pnl.to_string(),
        "orders": result.total_orders,
        "positions": result.total_positions,
        "returns": analyzer.returns(),
        "position_returns": analyzer.position_returns(),
        "snapshots": snapshots.iter().map(|snapshot| {
            let mut value = serde_json::to_value(snapshot).unwrap();
            value.as_object_mut().unwrap().remove("event_id");
            value
        }).collect::<Vec<_>>(),
        "stats_returns": normalize_stats(&result.stats_returns),
        "stats_general": normalize_stats(&result.stats_general),
        "stats_pnls": result.stats_pnls.iter().map(|(currency, stats)| (currency.clone(), normalize_stats(stats))).collect::<BTreeMap<_, _>>(),
    })
}

fn assert_balances(account: &AccountAny, balance: Decimal, multi_currency: bool) {
    let balances = account.balances();
    assert_eq!(balances.len(), if multi_currency { 2 } else { 1 });
    let usdt = &balances[&Currency::USDT()];
    assert_eq!(usdt.total.as_decimal(), balance);
    assert_eq!(usdt.free.as_decimal(), balance);
    assert_eq!(usdt.locked.as_decimal(), Decimal::ZERO);
    assert_eq!(
        account.starting_balances()[&Currency::USDT()].as_decimal(),
        dec!(1_000_000)
    );

    if multi_currency {
        let eur = &balances[&Currency::EUR()];
        assert_eq!(eur.total.as_decimal(), dec!(1234.56));
        assert_eq!(eur.free.as_decimal(), dec!(1234.56));
        assert_eq!(eur.locked.as_decimal(), Decimal::ZERO);
    }
}

fn assert_fills(engine: &BacktestEngine, completed_pairs: usize) -> Decimal {
    let cache_rc = engine.kernel().cache();
    let cache = cache_rc.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), completed_pairs * 2);
    assert!(
        orders
            .iter()
            .all(|order| order.status() == OrderStatus::Filled)
    );
    let mut commission = Decimal::ZERO;

    for order in orders {
        for event in order.events() {
            let OrderEventAny::Filled(fill) = event else {
                continue;
            };

            let fee = fill.commission.unwrap();
            assert_eq!(fee.currency, Currency::USDT());
            assert_eq!(
                fee.as_decimal(),
                if fill.order_side == OrderSide::Buy {
                    dec!(0.100)
                } else {
                    dec!(0.102)
                }
            );
            commission += fee.as_decimal();
        }
    }

    assert_eq!(commission, dec!(0.202) * Decimal::from(completed_pairs));
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    let mut positions = cache
        .positions(None, None, None, None, None)
        .into_iter()
        .map(|position| position.cloned())
        .collect::<Vec<_>>();
    let closed = positions
        .iter()
        .flat_map(|position| cache.position_snapshots(Some(&position.id), None))
        .collect::<Vec<_>>();
    positions.extend(closed);
    assert_eq!(positions.len(), completed_pairs);

    for position in positions {
        assert_eq!(position.realized_pnl.unwrap().as_decimal(), dec!(1.798));
        assert_eq!(position.commissions(), vec![Money::from("0.202 USDT")]);
    }

    drop(cache);

    commission
}

fn assert_snapshots(
    snapshots: &[PortfolioSnapshot],
    days: usize,
    pairs: usize,
    equity_curve: bool,
) -> BTreeMap<UnixNanos, f64> {
    if !equity_curve {
        assert!(snapshots.is_empty());
        return BTreeMap::new();
    }

    assert_eq!(snapshots.len(), days + 1);

    for (index, snapshot) in snapshots.iter().enumerate() {
        let earned_days = (0..index).filter(|day| day % 3 != 1).count();
        let equity = dec!(1_000_000) + dec!(1.798) * Decimal::from(earned_days * pairs);

        let expected_ts = if index == days {
            START_NS + days as u64 * DAY_NS - SECOND_NS
        } else {
            START_NS + index as u64 * DAY_NS
        };

        assert_eq!(snapshot.ts_event, UnixNanos::new(expected_ts));
        assert!(snapshot.unpriced_instruments.is_empty());
        assert_eq!(
            snapshot
                .total_equity
                .iter()
                .find(|money| money.currency == Currency::USDT())
                .unwrap()
                .as_decimal(),
            equity,
        );
    }

    let mut expected_returns = BTreeMap::new();
    let mut previous = dec!(1_000_000);
    for day in 0..days {
        let current = previous
            + if day % 3 == 1 {
                Decimal::ZERO
            } else {
                dec!(1.798) * Decimal::from(pairs)
            };

        expected_returns.insert(
            UnixNanos::new(START_NS + day as u64 * DAY_NS),
            current.to_f64().unwrap() / previous.to_f64().unwrap() - 1.0,
        );
        previous = current;
    }

    expected_returns
}

fn account(engine: &BacktestEngine) -> AccountAny {
    engine
        .kernel()
        .cache()
        .borrow()
        .accounts_all_owned()
        .pop()
        .unwrap()
}

fn report_events(events: &[AccountState]) -> Vec<Value> {
    events
        .iter()
        .map(|event| {
            let mut value = serde_json::to_value(event).unwrap();
            value.as_object_mut().unwrap().remove("event_id");
            value
        })
        .collect()
}

fn retention_engine(
    days: usize,
    pairs: usize,
    lookback: Option<u32>,
    multi_currency: bool,
    equity_curve: bool,
) -> BacktestEngine {
    assert!(days > 0 && pairs > 0 && pairs <= 3_000);
    let mut engine = retention_venue(lookback, multi_currency, equity_curve);
    let instrument_id = crypto_perpetual_ethusdt().id;
    engine
        .add_strategy(RetentionOrders::new(instrument_id, pairs, 60))
        .unwrap();
    let mut data = Vec::new();
    for day in 0..days {
        data.push(retention_quote(instrument_id, day, 0, "100.00"));
        if day % 3 != 1 {
            for index in 0..pairs * 2 {
                let price = if index % 2 == 0 { "100.00" } else { "102.00" };
                data.push(retention_quote(instrument_id, day, 60 + index, price));
            }
        }

        data.push(retention_quote(instrument_id, day, 86_399, "102.00"));
    }

    engine.add_data(data, None, true, true).unwrap();
    engine
}

fn retention_venue(
    lookback: Option<u32>,
    multi_currency: bool,
    equity_curve: bool,
) -> BacktestEngine {
    let instrument = crypto_perpetual_ethusdt();
    let instrument_id = instrument.id;

    let config = BacktestEngineConfig {
        logging: LoggerConfig::from_spec("bypass_logging").unwrap(),
        exec_engine: lookback.map(|lookback| ExecutionEngineConfig {
            purge_account_events_interval_mins: Some(60),
            purge_account_events_lookback_mins: Some(lookback),
            ..Default::default()
        }),
        portfolio: Some(PortfolioConfig {
            equity_curve,
            ..Default::default()
        }),
        bypass_logging: true,
        ..Default::default()
    };

    let mut engine = BacktestEngine::new(config).unwrap();
    let mut balances = vec![Money::from("1_000_000 USDT")];
    if multi_currency {
        balances.push(Money::from("1234.56 EUR"));
    }

    let venue = SimulatedVenueConfig::builder()
        .venue(instrument_id.venue)
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(balances)
        .fee_model(
            FeeModelAny::MakerTaker(MakerTakerFeeModel::new(dec!(0.001), dec!(0.001))).into(),
        )
        .build()
        .unwrap();
    engine.add_venue(venue).unwrap();
    engine
        .add_instrument(&InstrumentAny::CryptoPerpetual(instrument))
        .unwrap();
    engine
}

fn retention_quote(instrument_id: InstrumentId, day: usize, second: usize, price: &str) -> Data {
    let timestamp = UnixNanos::new(START_NS + day as u64 * DAY_NS + second as u64 * SECOND_NS);
    let price = Price::from(price);
    Data::Quote(QuoteTick::new(
        instrument_id,
        price,
        price,
        Quantity::from("100.000"),
        Quantity::from("100.000"),
        timestamp,
        timestamp,
    ))
}

struct RetentionOrders {
    core: StrategyCore,
    instrument_id: InstrumentId,
    pairs: usize,
    start_second: u64,
}

impl RetentionOrders {
    fn new(instrument_id: InstrumentId, pairs: usize, start_second: u64) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("RETENTION-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };

        Self {
            core: StrategyCore::new(config),
            instrument_id,
            pairs,
            start_second,
        }
    }
}

nautilus_strategy!(RetentionOrders);

impl DataActor for RetentionOrders {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        let elapsed = quote.ts_event.as_u64() - START_NS;
        let day = elapsed / DAY_NS;
        let second = elapsed % DAY_NS / SECOND_NS;

        if day % 3 == 1
            || second < self.start_second
            || second >= self.start_second + self.pairs as u64 * 2
        {
            return Ok(());
        }

        let side = if (second - self.start_second).is_multiple_of(2) {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let order = self.order().market(
            self.instrument_id,
            side,
            Quantity::from("1.000"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)
    }
}

impl Debug for RetentionOrders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RetentionOrders)).finish()
    }
}
