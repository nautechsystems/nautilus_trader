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

//! Benchmarks for deriving a fill from a repeated Betfair order update.
//!
//! The common stream update leaves matched size unchanged. That path must stay cheap when the
//! average price is also unchanged.
//!
//! Run with `cargo bench -p nautilus-betfair --bench fill_tracker_criterion`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use nautilus_betfair::{
    common::enums::{StreamingOrderStatus, StreamingOrderType, StreamingSide},
    stream::{messages::UnmatchedOrder, parse::FillTracker},
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    types::Currency,
};
use rust_decimal::Decimal;

fn order(sm: u64, avp: u64) -> UnmatchedOrder {
    UnmatchedOrder {
        id: "bet-1".to_string(),
        p: Decimal::from(12),
        s: Decimal::from(4),
        side: StreamingSide::Back,
        status: StreamingOrderStatus::Executable,
        pt: None,
        ot: StreamingOrderType::Limit,
        pd: 0,
        bsp: None,
        rfo: None,
        rfs: None,
        rc: None,
        rac: None,
        md: None,
        cd: None,
        ld: None,
        avp: Some(Decimal::from(avp)),
        sm: Some(Decimal::from(sm)),
        sr: None,
        sl: None,
        sc: None,
        sv: None,
        lsrc: None,
    }
}

fn seeded_tracker() -> FillTracker {
    let mut tracker = FillTracker::new();
    report(&mut tracker, &order(2, 12));
    tracker
}

fn report(tracker: &mut FillTracker, order: &UnmatchedOrder) {
    tracker.maybe_fill_report(
        order,
        Decimal::from(4),
        InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
        AccountId::from("BETFAIR-001"),
        Currency::GBP(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
}

fn bench_order_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("betfair/fill_tracker");
    let seeded = seeded_tracker();
    let same = order(2, 12);
    let increased = order(3, 11);

    group.bench_function("unchanged_sm_same_avp", |b| {
        b.iter_batched_ref(
            || seeded.clone(),
            |tracker| {
                black_box(report(tracker, black_box(&same)));
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("increased_sm", |b| {
        b.iter_batched_ref(
            || seeded.clone(),
            |tracker| {
                black_box(report(tracker, black_box(&increased)));
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_order_update);
criterion_main!(benches);
