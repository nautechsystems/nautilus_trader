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

use iai::black_box;
use nautilus_common::msgbus::matching::{is_matching, is_matching_backtracking};

fn bench_matching_exact() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.quotes.BINANCE.ETHUSDT";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_star_end() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.quotes.BINANCE.*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_star_middle() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.ETHUSDT";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_multi_star() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_question() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.quotes.BINANCE.ETHUS?";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_multi_question() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.quotes.BINANCE.ETH???";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_mixed() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.*.BINANCE.ETH???";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_realistic() {
    let topic = b"data.trades.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.ETH*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_no_match() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.trades.BYBIT.*";
    black_box(is_matching(topic, pattern));
}

fn bench_mstr_matching_exact() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSDT".into(),
        "data.quotes.BINANCE.ETHUSDT".into(),
    ));
}

fn bench_mstr_matching_star_middle() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSDT".into(),
        "data.*.BINANCE.ETHUSDT".into(),
    ));
}

fn bench_mstr_matching_mixed() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSD".into(),
        "data.*.BINANCE.ETH???".into(),
    ));
}

fn bench_mstr_matching_realistic() {
    black_box(is_matching_backtracking(
        "data.trades.BINANCE.ETHUSDT".into(),
        "data.*.BINANCE.ETH*".into(),
    ));
}

iai::main!(
    bench_matching_exact,
    bench_matching_star_end,
    bench_matching_star_middle,
    bench_matching_multi_star,
    bench_matching_question,
    bench_matching_multi_question,
    bench_matching_mixed,
    bench_matching_realistic,
    bench_matching_no_match,
    bench_mstr_matching_exact,
    bench_mstr_matching_star_middle,
    bench_mstr_matching_mixed,
    bench_mstr_matching_realistic,
);
