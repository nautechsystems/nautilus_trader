// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nautilus_common::msgbus::matching::is_matching_backtracking;
use rand::{Rng, SeedableRng, rngs::StdRng};
use regex::Regex;
use ustr::Ustr;

fn create_topics(n: usize, rng: &mut StdRng) -> Vec<Ustr> {
    let cat = ["data", "info", "order"];
    let model = ["quotes", "trades", "orderbooks", "depths"];
    let venue = ["BINANCE", "BYBIT", "OKX", "FTX", "KRAKEN"];
    let instrument = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

    let mut topics = Vec::new();
    for _ in 0..n {
        let cat = cat[rng.random_range(0..cat.len())];
        let model = model[rng.random_range(0..model.len())];
        let venue = venue[rng.random_range(0..venue.len())];
        let instrument = instrument[rng.random_range(0..instrument.len())];
        topics.push(Ustr::from(&format!("{cat}.{model}.{venue}.{instrument}")));
    }
    topics
}

fn bench_matching(c: &mut Criterion) {
    let pattern = "data.*.BINANCE.ETH???";

    {
        let mut rng = StdRng::seed_from_u64(42);
        let mut regex_group = c.benchmark_group("Regex matching");

        for ele in [1, 10, 100, 1000] {
            let topics = create_topics(ele, &mut rng);

            regex_group.bench_function(format!("{ele} topics"), |b| {
                b.iter(|| {
                    let regex = Regex::new(pattern).unwrap();
                    for topic in &topics {
                        black_box(regex.is_match(topic));
                    }
                });
            });
        }

        regex_group.finish();
    }

    {
        let mut rng = StdRng::seed_from_u64(42);
        let mut iter_group = c.benchmark_group("Iterative backtracking matching");
        let pattern = pattern.into();

        for ele in [1, 10, 100, 1000] {
            let topics = create_topics(ele, &mut rng);

            iter_group.bench_function(format!("{ele} topics"), |b| {
                b.iter(|| {
                    for topic in &topics {
                        black_box(is_matching_backtracking(topic.into(), pattern));
                    }
                });
            });
        }

        iter_group.finish();
    }
}

criterion_group!(benches, bench_matching);
criterion_main!(benches);
