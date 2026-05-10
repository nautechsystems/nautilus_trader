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

//! Benchmarks comparing Any-based message handling vs typed message handling.
//!
//! Focus areas:
//! 1. Isolate bus overhead with noop handlers
//! 2. Compare TopicRouter<T> vs Any-based routing
//! 3. Test with realistic topic patterns (exact + wildcards)
//! 4. Large message counts for stable timings

use std::{
    any::Any,
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::msgbus::{
    Handler, MStr, Pattern, Topic, TypedHandler, typed_handler::shareable_handler,
    typed_router::TopicRouter,
};
use nautilus_model::data::QuoteTick;
use ustr::Ustr;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct NoopAnyHandler {
    id: Ustr,
}

impl Handler<dyn Any> for NoopAnyHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        // Noop - but prevent optimization
        black_box(message);
    }
}

#[derive(Clone)]
struct NoopTypedHandler {
    id: Ustr,
}

impl Handler<QuoteTick> for NoopTypedHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, quote: &QuoteTick) {
        black_box(quote);
    }
}

// --
// Counting handlers - minimal work to prevent dead code elimination
// --

struct CountingAnyHandler {
    id: Ustr,
}

impl Handler<dyn Any> for CountingAnyHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        let quote = message.downcast_ref::<QuoteTick>().unwrap();
        COUNTER.fetch_add(quote.bid_price.raw as u64, Ordering::Relaxed);
    }
}

#[derive(Clone)]
struct CountingTypedHandler {
    id: Ustr,
}

impl Handler<QuoteTick> for CountingTypedHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, quote: &QuoteTick) {
        COUNTER.fetch_add(quote.bid_price.raw as u64, Ordering::Relaxed);
    }
}

// --
// Simulate Any-based topic routing (like the old MessageBus.topics)
// --

use std::rc::Rc;

use indexmap::IndexMap;
use nautilus_common::msgbus::{ShareableMessageHandler, matching::is_matching_backtracking};

struct AnyTopicRouter {
    subscriptions: Vec<(MStr<Pattern>, ShareableMessageHandler)>,
    topics: IndexMap<MStr<Topic>, Vec<ShareableMessageHandler>>,
}

impl AnyTopicRouter {
    fn new() -> Self {
        Self {
            subscriptions: Vec::new(),
            topics: IndexMap::new(),
        }
    }

    fn subscribe(&mut self, pattern: MStr<Pattern>, handler: ShareableMessageHandler) {
        // Add to existing matching topics
        for (topic, handlers) in &mut self.topics {
            if is_matching_backtracking(*topic, pattern) {
                handlers.push(handler.clone());
            }
        }
        self.subscriptions.push((pattern, handler));
    }

    fn publish(&mut self, topic: MStr<Topic>, message: &dyn Any) {
        let handlers = self.topics.entry(topic).or_insert_with(|| {
            self.subscriptions
                .iter()
                .filter(|(pattern, _)| is_matching_backtracking(topic, *pattern))
                .map(|(_, handler)| handler.clone())
                .collect()
        });

        for handler in handlers {
            handler.0.handle(message);
        }
    }
}

// --
// Benchmark: Noop handler dispatch overhead
// --

fn bench_noop_dispatch(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("Noop handler dispatch");
    group.throughput(Throughput::Elements(1));

    group.bench_function("Any-based", |b| {
        let handler = NoopAnyHandler {
            id: Ustr::from("noop"),
        };
        b.iter(|| handler.handle(black_box(&quote as &dyn Any)));
    });

    group.bench_function("Typed", |b| {
        let handler = NoopTypedHandler {
            id: Ustr::from("noop"),
        };
        b.iter(|| handler.handle(black_box(&quote)));
    });

    group.bench_function("Typed via TypedHandler", |b| {
        let handler = TypedHandler::new(NoopTypedHandler {
            id: Ustr::from("noop"),
        });
        b.iter(|| handler.handle(black_box(&quote)));
    });

    group.finish();
}

// --
// Benchmark: Full router publish path (the key comparison)
// --

fn bench_router_publish(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("Router publish (single topic)");
    group.throughput(Throughput::Elements(1));

    // Any-based router with single exact topic
    group.bench_function("Any-based router", |b| {
        let mut router = AnyTopicRouter::new();
        let handler = shareable_handler(Rc::new(CountingAnyHandler {
            id: Ustr::from("handler"),
        }));
        let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.BTCUSDT");
        router.subscribe(pattern, handler);

        let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");

        // Warm cache
        router.publish(topic, &quote as &dyn Any);

        b.iter(|| {
            router.publish(black_box(topic), black_box(&quote as &dyn Any));
        });
    });

    // Typed router with single exact topic
    group.bench_function("Typed router", |b| {
        let mut router = TopicRouter::<QuoteTick>::new();
        let handler = TypedHandler::new(CountingTypedHandler {
            id: Ustr::from("handler"),
        });
        let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.BTCUSDT");
        router.subscribe(pattern, handler, 0);

        let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");

        // Warm cache
        router.publish(topic, &quote);

        b.iter(|| {
            router.publish(black_box(topic), black_box(&quote));
        });
    });

    group.finish();
}

// --
// Benchmark: Multiple subscribers (realistic pub/sub)
// --

fn bench_router_multiple_subscribers(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("Router with multiple subscribers");

    for sub_count in [1, 5, 10] {
        group.throughput(Throughput::Elements(sub_count as u64));

        group.bench_with_input(
            BenchmarkId::new("Any-based", sub_count),
            &sub_count,
            |b, &count| {
                let mut router = AnyTopicRouter::new();
                let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.BTCUSDT");

                for i in 0..count {
                    let handler = shareable_handler(Rc::new(CountingAnyHandler {
                        id: Ustr::from(&format!("handler_{i}")),
                    }));
                    router.subscribe(pattern, handler);
                }

                let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.publish(topic, &quote as &dyn Any); // warm

                b.iter(|| {
                    router.publish(black_box(topic), black_box(&quote as &dyn Any));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("Typed", sub_count),
            &sub_count,
            |b, &count| {
                let mut router = TopicRouter::<QuoteTick>::new();
                let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.BTCUSDT");

                for i in 0..count {
                    let handler = TypedHandler::new(CountingTypedHandler {
                        id: Ustr::from(&format!("handler_{i}")),
                    });
                    router.subscribe(pattern, handler, 0);
                }

                let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.publish(topic, &quote); // warm

                b.iter(|| {
                    router.publish(black_box(topic), black_box(&quote));
                });
            },
        );
    }

    group.finish();
}

// --
// Benchmark: Wildcard pattern matching
// --

fn bench_router_wildcards(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("Router with wildcard patterns");
    group.throughput(Throughput::Elements(1));

    // Pattern: data.quotes.*.BTCUSDT (matches any venue)
    group.bench_function("Any-based (wildcard)", |b| {
        let mut router = AnyTopicRouter::new();
        let handler = shareable_handler(Rc::new(CountingAnyHandler {
            id: Ustr::from("handler"),
        }));
        let pattern: MStr<Pattern> = MStr::from("data.quotes.*.BTCUSDT");
        router.subscribe(pattern, handler);

        let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
        router.publish(topic, &quote as &dyn Any); // warm

        b.iter(|| {
            router.publish(black_box(topic), black_box(&quote as &dyn Any));
        });
    });

    group.bench_function("Typed (wildcard)", |b| {
        let mut router = TopicRouter::<QuoteTick>::new();
        let handler = TypedHandler::new(CountingTypedHandler {
            id: Ustr::from("handler"),
        });
        let pattern: MStr<Pattern> = MStr::from("data.quotes.*.BTCUSDT");
        router.subscribe(pattern, handler, 0);

        let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
        router.publish(topic, &quote); // warm

        b.iter(|| {
            router.publish(black_box(topic), black_box(&quote));
        });
    });

    group.finish();
}

// --
// Benchmark: Cold-path publish (first time topic seen, cache miss + pattern scan)
// --

fn bench_cold_path_publish(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("Cold-path publish (cache miss)");
    group.throughput(Throughput::Elements(1));

    // Any-based: first publish to new topic triggers pattern matching
    group.bench_function("Any-based", |b| {
        b.iter_with_setup(
            || {
                let mut router = AnyTopicRouter::new();
                let handler = shareable_handler(Rc::new(CountingAnyHandler {
                    id: Ustr::from("handler"),
                }));
                let pattern: MStr<Pattern> = MStr::from("data.quotes.*.*");
                router.subscribe(pattern, handler);
                router
            },
            |mut router| {
                let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.publish(black_box(topic), black_box(&quote as &dyn Any));
            },
        );
    });

    // Typed: first publish to new topic triggers pattern matching
    group.bench_function("Typed", |b| {
        b.iter_with_setup(
            || {
                let mut router = TopicRouter::<QuoteTick>::new();
                let handler = TypedHandler::new(CountingTypedHandler {
                    id: Ustr::from("handler"),
                });
                let pattern: MStr<Pattern> = MStr::from("data.quotes.*.*");
                router.subscribe(pattern, handler, 0);
                router
            },
            |mut router| {
                let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.publish(black_box(topic), black_box(&quote));
            },
        );
    });

    group.finish();
}

// --
// Benchmark: High volume (1M messages)
// --

fn bench_high_volume(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("High volume throughput");

    for msg_count in [100_000u64, 1_000_000] {
        group.throughput(Throughput::Elements(msg_count));

        group.bench_with_input(
            BenchmarkId::new("Any-based", msg_count),
            &msg_count,
            |b, &count| {
                let mut router = AnyTopicRouter::new();
                let handler = shareable_handler(Rc::new(CountingAnyHandler {
                    id: Ustr::from("handler"),
                }));
                let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.subscribe(pattern, handler);

                let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.publish(topic, &quote as &dyn Any); // warm

                b.iter(|| {
                    for _ in 0..count {
                        router.publish(topic, &quote as &dyn Any);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("Typed", msg_count),
            &msg_count,
            |b, &count| {
                let mut router = TopicRouter::<QuoteTick>::new();
                let handler = TypedHandler::new(CountingTypedHandler {
                    id: Ustr::from("handler"),
                });
                let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.subscribe(pattern, handler, 0);

                let topic: MStr<Topic> = MStr::from("data.quotes.BINANCE.BTCUSDT");
                router.publish(topic, &quote); // warm

                b.iter(|| {
                    for _ in 0..count {
                        router.publish(topic, &quote);
                    }
                });
            },
        );
    }

    group.finish();
}

// --
// Benchmark: Mixed topics (realistic workload)
// --

fn bench_mixed_topics(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("Mixed topics workload");

    // Setup: Subscribe to multiple instruments with wildcard
    let instruments = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT"];
    let topics: Vec<MStr<Topic>> = instruments
        .iter()
        .map(|i| MStr::from(&format!("data.quotes.BINANCE.{i}")))
        .collect();

    group.throughput(Throughput::Elements(instruments.len() as u64));

    group.bench_function("Any-based (4 instruments)", |b| {
        let mut router = AnyTopicRouter::new();

        // Wildcard subscription for all BINANCE quotes
        let handler = shareable_handler(Rc::new(CountingAnyHandler {
            id: Ustr::from("handler"),
        }));
        let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.*");
        router.subscribe(pattern, handler);

        // Warm all topics
        for topic in &topics {
            router.publish(*topic, &quote as &dyn Any);
        }

        b.iter(|| {
            for topic in &topics {
                router.publish(black_box(*topic), black_box(&quote as &dyn Any));
            }
        });
    });

    group.bench_function("Typed (4 instruments)", |b| {
        let mut router = TopicRouter::<QuoteTick>::new();

        let handler = TypedHandler::new(CountingTypedHandler {
            id: Ustr::from("handler"),
        });
        let pattern: MStr<Pattern> = MStr::from("data.quotes.BINANCE.*");
        router.subscribe(pattern, handler, 0);

        // Warm all topics
        for topic in &topics {
            router.publish(*topic, &quote);
        }

        b.iter(|| {
            for topic in &topics {
                router.publish(black_box(*topic), black_box(&quote));
            }
        });
    });

    group.finish();
}

// --
// Benchmark: RefCell overhead (applies equally to both paths)
// --

use std::cell::RefCell;

fn bench_refcell_overhead(c: &mut Criterion) {
    let quote = QuoteTick::default();

    let mut group = c.benchmark_group("RefCell overhead");
    group.throughput(Throughput::Elements(1));

    // Direct router access (no RefCell)
    group.bench_function("TopicRouter direct", |b| {
        let mut router = TopicRouter::<QuoteTick>::new();
        let handler = TypedHandler::new(CountingTypedHandler {
            id: Ustr::from("handler"),
        });
        let pattern: MStr<Pattern> = "data.quotes.BINANCE.BTCUSDT".into();
        router.subscribe(pattern, handler, 0);

        let topic: MStr<Topic> = "data.quotes.BINANCE.BTCUSDT".into();
        router.publish(topic, &quote); // warm

        b.iter(|| {
            router.publish(black_box(topic), black_box(&quote));
        });
    });

    // Router wrapped in RefCell (simulates MessageBus access pattern)
    group.bench_function("TopicRouter via RefCell", |b| {
        let router = RefCell::new(TopicRouter::<QuoteTick>::new());
        let handler = TypedHandler::new(CountingTypedHandler {
            id: Ustr::from("handler"),
        });
        let pattern: MStr<Pattern> = "data.quotes.BINANCE.BTCUSDT".into();
        router.borrow_mut().subscribe(pattern, handler, 0);

        let topic: MStr<Topic> = "data.quotes.BINANCE.BTCUSDT".into();
        router.borrow_mut().publish(topic, &quote); // warm

        b.iter(|| {
            router
                .borrow_mut()
                .publish(black_box(topic), black_box(&quote));
        });
    });

    // Router wrapped in RefCell + thread_local (full MessageBus pattern)
    group.bench_function("TopicRouter via thread_local + RefCell", |b| {
        thread_local! {
            static ROUTER: RefCell<TopicRouter<QuoteTick>> = RefCell::new(TopicRouter::new());
        }

        ROUTER.with(|router| {
            let handler = TypedHandler::new(CountingTypedHandler {
                id: Ustr::from("handler"),
            });
            let pattern: MStr<Pattern> = "data.quotes.BINANCE.BTCUSDT".into();
            router.borrow_mut().subscribe(pattern, handler, 0);

            let topic: MStr<Topic> = "data.quotes.BINANCE.BTCUSDT".into();
            router.borrow_mut().publish(topic, &quote); // warm

            b.iter(|| {
                router
                    .borrow_mut()
                    .publish(black_box(topic), black_box(&quote));
            });
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_noop_dispatch,
    bench_router_publish,
    bench_cold_path_publish,
    bench_router_multiple_subscribers,
    bench_router_wildcards,
    bench_high_volume,
    bench_mixed_topics,
    bench_refcell_overhead,
);
criterion_main!(benches);
