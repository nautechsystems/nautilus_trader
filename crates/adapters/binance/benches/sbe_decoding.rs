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

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_binance::spot::{
    http::parse::{decode_agg_trades, decode_depth, decode_trades},
    sbe::{
        generated::{
            SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, agg_trades_response_codec, depth_response_codec,
            trades_response_codec,
        },
        stream::{STREAM_SCHEMA_ID, STREAM_SCHEMA_VERSION, template_id},
    },
    websocket::streams::parse::{MarketDataMessage, decode_market_data},
};

fn bench_streams(c: &mut Criterion) {
    // Includes production routing, fixed-field decoding, allocation, symbol interning, and drop
    let mut group = c.benchmark_group("binance_sbe/stream");

    for (name, template, counts) in [
        (
            "trades",
            template_id::TRADES_STREAM_EVENT,
            &[1, 16, 256][..],
        ),
        (
            "depth_snapshot",
            template_id::DEPTH_SNAPSHOT_STREAM_EVENT,
            &[20][..],
        ),
        (
            "depth_diff",
            template_id::DEPTH_DIFF_STREAM_EVENT,
            &[0, 1, 20, 256][..],
        ),
    ] {
        for &count in counts {
            let bytes = stream_message(template, count);
            match decode_market_data(&bytes).unwrap() {
                MarketDataMessage::Trades(event) => {
                    assert_eq!(event.trades.len(), usize::from(count));
                    assert_eq!(event.symbol, "BTCUSDT");
                    assert_eq!(event.trades[0].id, 1);
                    assert_eq!(event.trades[usize::from(count) - 1].id, i64::from(count));
                }
                MarketDataMessage::DepthSnapshot(event) => {
                    assert_eq!(event.bids.len(), usize::from(count));
                    assert_eq!(event.asks.len(), usize::from(count));
                    assert_eq!(event.symbol, "BTCUSDT");
                }
                MarketDataMessage::DepthDiff(event) => {
                    assert_eq!(event.bids.len(), usize::from(count));
                    assert_eq!(event.asks.len(), usize::from(count));
                    assert_eq!(event.symbol, "BTCUSDT");
                }
                MarketDataMessage::BestBidAsk(_) => unreachable!(),
            }

            let entries = if template == template_id::TRADES_STREAM_EVENT {
                u64::from(count)
            } else {
                u64::from(count) * 2
            };

            group.throughput(Throughput::Elements(entries));
            group.bench_with_input(BenchmarkId::new(name, count), &bytes, |b, bytes| {
                b.iter(|| black_box(decode_market_data(black_box(bytes)).unwrap()));
            });
        }
    }

    group.finish();
}

fn bench_http(c: &mut Criterion) {
    let mut group = c.benchmark_group("binance_sbe/http");

    for count in [0, 20, 256] {
        for (name, template) in [
            ("depth", depth_response_codec::SBE_TEMPLATE_ID),
            ("trades", trades_response_codec::SBE_TEMPLATE_ID),
            ("agg_trades", agg_trades_response_codec::SBE_TEMPLATE_ID),
        ] {
            let bytes = http_message(template, count);
            let entries = if template == depth_response_codec::SBE_TEMPLATE_ID {
                u64::from(count) * 2
            } else {
                u64::from(count)
            };

            group.throughput(Throughput::Elements(entries));
            group.bench_with_input(
                BenchmarkId::new(name, count),
                &bytes,
                |b, bytes| match template {
                    depth_response_codec::SBE_TEMPLATE_ID => {
                        let value = decode_depth(bytes).unwrap();
                        assert_eq!(value.bids.len(), usize::from(count));
                        assert_eq!(value.asks.len(), usize::from(count));
                        b.iter(|| black_box(decode_depth(black_box(bytes)).unwrap()));
                    }
                    trades_response_codec::SBE_TEMPLATE_ID => {
                        assert_eq!(
                            decode_trades(bytes).unwrap().trades.len(),
                            usize::from(count)
                        );
                        b.iter(|| black_box(decode_trades(black_box(bytes)).unwrap()));
                    }
                    agg_trades_response_codec::SBE_TEMPLATE_ID => {
                        assert_eq!(
                            decode_agg_trades(bytes).unwrap().trades.len(),
                            usize::from(count)
                        );
                        b.iter(|| black_box(decode_agg_trades(black_box(bytes)).unwrap()));
                    }
                    _ => unreachable!(),
                },
            );
        }
    }

    group.finish();
}

fn stream_message(template: u16, count: u16) -> Vec<u8> {
    let diff = template == template_id::DEPTH_DIFF_STREAM_EVENT;
    let mut bytes = message_header(
        if diff { 26 } else { 18 },
        template,
        STREAM_SCHEMA_ID,
        STREAM_SCHEMA_VERSION,
    );
    bytes.extend_from_slice(&1_700_000_000_000_000i64.to_le_bytes());
    bytes.extend_from_slice(&1_700_000_000_000_001i64.to_le_bytes());
    if diff {
        bytes.extend_from_slice(&1_700_000_000_000_002i64.to_le_bytes());
    }

    bytes.extend_from_slice(&[(-2i8) as u8, (-8i8) as u8]);
    if template == template_id::TRADES_STREAM_EVENT {
        bytes.extend_from_slice(&25u16.to_le_bytes());
        bytes.extend_from_slice(&u32::from(count).to_le_bytes());
        for index in 0..count {
            bytes.extend_from_slice(&(i64::from(index) + 1).to_le_bytes());
            bytes.extend_from_slice(&(4_200_000 + i64::from(index)).to_le_bytes());
            bytes.extend_from_slice(&(100_000_000 + i64::from(index)).to_le_bytes());
            bytes.push(u8::from(index % 2 == 0));
        }
    } else {
        for side in [0, 1] {
            bytes.extend_from_slice(&16u16.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            append_levels(&mut bytes, count, side);
        }
    }

    bytes.push(7);
    bytes.extend_from_slice(b"BTCUSDT");
    bytes
}

fn http_message(template: u16, count: u16) -> Vec<u8> {
    let depth = template == depth_response_codec::SBE_TEMPLATE_ID;
    let mut bytes = message_header(
        if depth { 10 } else { 2 },
        template,
        SBE_SCHEMA_ID,
        SBE_SCHEMA_VERSION,
    );

    if depth {
        bytes.extend_from_slice(&123_456_789i64.to_le_bytes());
    }

    bytes.extend_from_slice(&[(-2i8) as u8, (-8i8) as u8]);

    if depth {
        for side in [0, 1] {
            bytes.extend_from_slice(&16u16.to_le_bytes());
            bytes.extend_from_slice(&u32::from(count).to_le_bytes());
            append_levels(&mut bytes, count, side);
        }
    } else {
        let fields = if template == trades_response_codec::SBE_TEMPLATE_ID {
            5u16
        } else {
            6
        };

        bytes.extend_from_slice(&(fields * 8 + 2).to_le_bytes());
        bytes.extend_from_slice(&u32::from(count).to_le_bytes());
        for index in 0..count {
            for field in 1..=fields {
                bytes.extend_from_slice(
                    &(1_000_000 + i64::from(index) * 10 + i64::from(field)).to_le_bytes(),
                );
            }

            bytes.extend_from_slice(&[u8::from(index % 2 == 0), 1]);
        }
    }

    bytes
}

fn message_header(block_length: u16, template: u16, schema: u16, version: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [block_length, template, schema, version] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    bytes
}

fn append_levels(bytes: &mut Vec<u8>, count: u16, side: i64) {
    for index in 0..count {
        bytes.extend_from_slice(&(4_200_000 + side * 1000 + i64::from(index)).to_le_bytes());
        bytes.extend_from_slice(&(100_000_000 + i64::from(index)).to_le_bytes());
    }
}

criterion_group!(benches, bench_streams, bench_http);
criterion_main!(benches);
