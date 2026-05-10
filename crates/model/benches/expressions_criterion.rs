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

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group};
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::SyntheticInstrument,
};

fn make_synth(components: &[&str], formula: &str) -> SyntheticInstrument {
    let ids: Vec<InstrumentId> = components.iter().map(|s| InstrumentId::from(*s)).collect();
    SyntheticInstrument::new(Symbol::from("BENCH"), 8, ids, formula, 0.into(), 0.into())
}

pub fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("expression/compile");

    group.bench_function("simple_avg", |b| {
        b.iter(|| {
            black_box(make_synth(
                black_box(&["BTC.BINANCE", "LTC.BINANCE"]),
                black_box("(BTC.BINANCE + LTC.BINANCE) / 2.0"),
            ))
        });
    });

    group.bench_function("weighted_4", |b| {
        b.iter(|| {
            black_box(make_synth(
                black_box(&["BTC.BINANCE", "ETH.BINANCE", "SOL.BINANCE", "ADA.BINANCE"]),
                black_box(
                    "BTC.BINANCE * 0.4 + ETH.BINANCE * 0.3 + SOL.BINANCE * 0.2 + ADA.BINANCE * 0.1",
                ),
            ))
        });
    });

    group.bench_function("conditional", |b| {
        b.iter(|| {
            black_box(make_synth(
                black_box(&["BTC.BINANCE", "ETH.BINANCE"]),
                black_box("if(BTC.BINANCE > ETH.BINANCE, BTC.BINANCE - ETH.BINANCE, ETH.BINANCE - BTC.BINANCE)"),
            ))
        });
    });

    group.bench_function("with_locals", |b| {
        b.iter(|| {
            black_box(make_synth(
                black_box(&["BTC.BINANCE", "ETH.BINANCE"]),
                black_box("spread = BTC.BINANCE - ETH.BINANCE; mid = (BTC.BINANCE + ETH.BINANCE) / 2.0; mid + spread / 2.0"),
            ))
        });
    });

    group.bench_function("hyphenated_ids", |b| {
        b.iter(|| {
            black_box(make_synth(
                black_box(&["ETH-USDT-SWAP.OKX", "ETH-USDC-PERP.HYPERLIQUID"]),
                black_box("(ETH-USDT-SWAP.OKX + ETH-USDC-PERP.HYPERLIQUID) / 2.0"),
            ))
        });
    });

    group.finish();
}

pub fn bench_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("expression/eval");

    let synth_avg = make_synth(
        &["BTC.BINANCE", "LTC.BINANCE"],
        "(BTC.BINANCE + LTC.BINANCE) / 2.0",
    );
    group.bench_function("simple_avg", |b| {
        b.iter(|| black_box(synth_avg.calculate(black_box(&[50000.0, 100.0])).unwrap()));
    });

    let synth_weighted = make_synth(
        &["BTC.BINANCE", "ETH.BINANCE", "SOL.BINANCE", "ADA.BINANCE"],
        "BTC.BINANCE * 0.4 + ETH.BINANCE * 0.3 + SOL.BINANCE * 0.2 + ADA.BINANCE * 0.1",
    );
    group.bench_function("weighted_4", |b| {
        b.iter(|| {
            black_box(
                synth_weighted
                    .calculate(black_box(&[50000.0, 3000.0, 150.0, 0.5]))
                    .unwrap(),
            )
        });
    });

    let synth_cond = make_synth(
        &["BTC.BINANCE", "ETH.BINANCE"],
        "if(BTC.BINANCE > ETH.BINANCE, BTC.BINANCE - ETH.BINANCE, ETH.BINANCE - BTC.BINANCE)",
    );
    group.bench_function("conditional", |b| {
        b.iter(|| black_box(synth_cond.calculate(black_box(&[50000.0, 3000.0])).unwrap()));
    });

    let synth_locals = make_synth(
        &["BTC.BINANCE", "ETH.BINANCE"],
        "spread = BTC.BINANCE - ETH.BINANCE; mid = (BTC.BINANCE + ETH.BINANCE) / 2.0; mid + spread / 2.0",
    );
    group.bench_function("with_locals", |b| {
        b.iter(|| {
            black_box(
                synth_locals
                    .calculate(black_box(&[50000.0, 3000.0]))
                    .unwrap(),
            )
        });
    });

    let synth_nested = make_synth(
        &["BTC.BINANCE", "ETH.BINANCE"],
        "max(min(BTC.BINANCE, ETH.BINANCE * 20), abs(BTC.BINANCE - ETH.BINANCE))",
    );
    group.bench_function("nested_calls", |b| {
        b.iter(|| {
            black_box(
                synth_nested
                    .calculate(black_box(&[50000.0, 3000.0]))
                    .unwrap(),
            )
        });
    });

    group.finish();
}

pub fn bench_eval_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("expression/eval_scaling");

    for n in [2, 4, 8] {
        let components: Vec<String> = (0..n).map(|i| format!("C{i}.VENUE")).collect();
        let component_refs: Vec<&str> = components.iter().map(String::as_str).collect();

        let terms: Vec<String> = components
            .iter()
            .map(|c| format!("{c} * {:.4}", 1.0 / n as f64))
            .collect();
        let formula = terms.join(" + ");

        let synth = make_synth(&component_refs, &formula);
        let inputs: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 10.0).collect();

        group.bench_with_input(BenchmarkId::new("weighted_sum", n), &inputs, |b, inputs| {
            b.iter(|| black_box(synth.calculate(black_box(inputs)).unwrap()));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_compile, bench_eval, bench_eval_scaling);
criterion::criterion_main!(benches);
