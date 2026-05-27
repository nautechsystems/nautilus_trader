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

//! Logging benchmarks for the producer-adjacent and writer-adjacent hot paths.
//!
//! These benches intentionally focus on reusable units instead of the global
//! logger singleton, so optimization experiments can be compared without
//! cross-benchmark state leaking through `log::set_logger`.

use std::{
    hint::black_box,
    io::{self, BufWriter, Write},
    sync::OnceLock,
    thread,
};

use ahash::AHashMap;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use log::{Level, LevelFilter, Log, kv::Value};
use nautilus_common::{
    enums::{LogColor, LogLevel},
    logging::{
        logger::{
            LogEvent, LogFields, LogLine, LogLineWrapper, Logger, LoggerConfig, should_filter_log,
        },
        writer::{FileWriter, FileWriterConfig, LogWriter},
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::TraderId;
use tempfile::{TempDir, tempdir};
use ustr::Ustr;

const TIMESTAMP_NS: u64 = 1_725_000_000_123_456_789;
const TRADER_ID: &str = "TRADER-001";
const INSTANCE_ID: &str = "INSTANCE-001";
const COMPONENT: &str = "nautilus_trader::adapters::binance::execution";
const MESSAGE: &str = "Order accepted: client_order_id=O-20260527-000001 venue_order_id=123456789";
const CLEAN_FILE_LINE: &str =
    "2026-05-27T10:00:00.123456789Z [INFO] TRADER-001.Component: clean log line\n";
const ANSI_FILE_LINE: &str =
    "\x1b[1m2026-05-27T10:00:00.123456789Z\x1b[0m [INFO] Component: colored\x1b[0m\n";
const FILTERED_COMPONENT: &str = "BenchFiltered";
const FILTERED_TARGET: &str = "BenchFiltered";
const COMPONENT_TARGETS_12: [&str; 12] = [
    "nautilus_trader::adapters::binance::execution",
    "nautilus_trader::adapters::bybit::execution",
    "nautilus_trader::adapters::okx::execution",
    "nautilus_trader::adapters::coinbase::execution",
    "nautilus_trader::adapters::kraken::execution",
    "nautilus_trader::adapters::bitmex::execution",
    "nautilus_trader::adapters::deribit::execution",
    "nautilus_trader::adapters::dydx::execution",
    "nautilus_trader::adapters::hyperliquid::execution",
    "nautilus_trader::adapters::databento::execution",
    "nautilus_trader::adapters::betfair::execution",
    "nautilus_trader::adapters::sandbox::execution",
];

static FILTERED_LOGGER_INIT: OnceLock<()> = OnceLock::new();

fn make_log_line(fields: LogFields, color: LogColor) -> LogLine {
    LogLine {
        timestamp: UnixNanos::from(TIMESTAMP_NS),
        level: Level::Info,
        color,
        component: Ustr::from(COMPONENT),
        message: MESSAGE.to_string(),
        fields,
    }
}

fn make_log_line_without_fields() -> LogLine {
    make_log_line(LogFields::new(), LogColor::Normal)
}

fn make_log_line_with_fields() -> LogLine {
    let mut fields = LogFields::new();
    fields.push((Ustr::from("venue"), "BINANCE".to_string()));
    fields.push((Ustr::from("symbol"), "ETHUSDT-PERP".to_string()));
    fields.push((Ustr::from("strategy_id"), "S-001".to_string()));
    fields.push((Ustr::from("latency_ns"), "7421".to_string()));

    make_log_line(fields, LogColor::Green)
}

fn make_log_line_with_two_fields() -> LogLine {
    let mut fields = LogFields::new();
    fields.push((Ustr::from("venue"), "BINANCE".to_string()));
    fields.push((Ustr::from("symbol"), "ETHUSDT-PERP".to_string()));

    make_log_line(fields, LogColor::Green)
}

fn make_file_writer(name: &str) -> (TempDir, FileWriter) {
    let dir = tempdir().expect("failed to create temporary benchmark directory");
    let config = FileWriterConfig::new(
        Some(dir.path().to_string_lossy().into_owned()),
        Some(name.to_string()),
        None,
        None,
    );

    let writer = FileWriter::new(
        TRADER_ID.to_string(),
        INSTANCE_ID.to_string(),
        config,
        LevelFilter::Debug,
        true,
    )
    .expect("failed to create benchmark file writer");

    (dir, writer)
}

fn init_filtered_global_logger() {
    FILTERED_LOGGER_INIT.get_or_init(|| {
        // Criterion runs these producer benches in one process, while `log` allows only one
        // global logger. Leak the guard intentionally so all producer cases share one stable
        // filtered logger config for the benchmark binary lifetime.
        let config = LoggerConfig::from_spec("stdout=Info;fileout=Off;BenchFiltered=Error")
            .expect("valid benchmark logger config");
        let guard = Logger::init_with_config(
            TraderId::from("BENCHER-001"),
            UUID4::new(),
            config,
            FileWriterConfig::default(),
        )
        .expect("failed to initialize benchmark logger");

        std::mem::forget(guard);
    });
}

fn make_direct_logger(config: LoggerConfig) -> (Logger, thread::JoinHandle<()>) {
    let (tx, rx) = std::sync::mpsc::channel::<LogEvent>();
    let logger = Logger::new_for_benchmark(config, tx);

    let drain = thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            black_box(event);
        }
    });

    (logger, drain)
}

fn log_direct_no_fields(logger: &Logger, target: &'static str, value: u64) {
    let args = format_args!("direct benchmark message {value}");
    let record = log::Record::builder()
        .args(args)
        .level(Level::Info)
        .target(target)
        .build();

    logger.log(&record);
}

fn log_direct_with_fields(
    logger: &Logger,
    target: &'static str,
    component: &'static str,
    value: u64,
) {
    let key_values = [
        ("component", Value::from(component)),
        ("venue", Value::from("BINANCE")),
        ("latency_ns", Value::from(value)),
    ];
    let args = format_args!("direct benchmark message {value}");
    let record = log::Record::builder()
        .args(args)
        .level(Level::Info)
        .target(target)
        .key_values(&key_values)
        .build();

    logger.log(&record);
}

fn bench_log_line_formatting(c: &mut Criterion) {
    let trader_id = Ustr::from(TRADER_ID);
    let no_fields = make_log_line_without_fields();
    let with_two_fields = make_log_line_with_two_fields();
    let with_fields = make_log_line_with_fields();

    let mut group = c.benchmark_group("logging/line_format");

    // Measures the cold plain-text formatting path with a fresh wrapper each iteration.
    // This covers timestamp conversion, capacity sizing, and String writes for stdout/file logs.
    group.bench_function("plain_cold_no_fields", |b| {
        b.iter_batched(
            || LogLineWrapper::new(no_fields.clone(), trader_id),
            |mut wrapper| black_box(wrapper.get_string().len()),
            BatchSize::SmallInput,
        );
    });

    // Measures cold plain-text formatting with 4 extra structured fields.
    // This shows how fields affect capacity sizing, SmallVec storage, and field appends.
    group.bench_function("plain_cold_4_fields", |b| {
        b.iter_batched(
            || LogLineWrapper::new(with_fields.clone(), trader_id),
            |mut wrapper| black_box(wrapper.get_string().len()),
            BatchSize::SmallInput,
        );
    });

    // Measures the middle case with 2 extra fields.
    // This avoids drawing conclusions from only the 0-field and 4-field endpoints.
    group.bench_function("plain_cold_2_fields", |b| {
        b.iter_batched(
            || LogLineWrapper::new(with_two_fields.clone(), trader_id),
            |mut wrapper| black_box(wrapper.get_string().len()),
            BatchSize::SmallInput,
        );
    });

    // Measures cold colored formatting, including ANSI prefixes/suffixes and color selection.
    // This represents the formatter cost for colored stdout/stderr output.
    group.bench_function("colored_cold_4_fields", |b| {
        b.iter_batched(
            || LogLineWrapper::new(with_fields.clone(), trader_id),
            |mut wrapper| black_box(wrapper.get_colored().len()),
            BatchSize::SmallInput,
        );
    });

    // Measures wrapper cache-hit cost.
    // Re-reading the plain string should be near-free when one line is written to multiple sinks.
    group.bench_function("plain_cached", |b| {
        let mut wrapper = LogLineWrapper::new(with_fields.clone(), trader_id);
        black_box(wrapper.get_string().len());

        b.iter(|| black_box(wrapper.get_string().len()));
    });

    // Measures cold JSON formatting without extra fields.
    // This isolates serde streaming, fixed-field serialization, and the trailing newline append.
    group.bench_function("json_cold_no_fields", |b| {
        b.iter_batched(
            || LogLineWrapper::new(no_fields.clone(), trader_id),
            |wrapper| black_box(wrapper.get_json().len()),
            BatchSize::SmallInput,
        );
    });

    // Measures cold JSON formatting with 4 extra fields.
    // This captures extra-field serialization, reserved-key filtering, and duplicate fallback checks.
    group.bench_function("json_cold_4_fields", |b| {
        b.iter_batched(
            || LogLineWrapper::new(with_fields.clone(), trader_id),
            |wrapper| black_box(wrapper.get_json().len()),
            BatchSize::SmallInput,
        );
    });

    // Measures the middle JSON case with 2 extra fields.
    // This checks that field-storage changes do not only optimize extreme inputs.
    group.bench_function("json_cold_2_fields", |b| {
        b.iter_batched(
            || LogLineWrapper::new(with_two_fields.clone(), trader_id),
            |wrapper| black_box(wrapper.get_json().len()),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_filtering(c: &mut Criterion) {
    let component = Ustr::from(COMPONENT);
    let mut component_levels = AHashMap::new();
    component_levels.insert(component, LevelFilter::Warn);

    let mut module_filters = vec![
        (Ustr::from("nautilus_trader::adapters"), LevelFilter::Info),
        (
            Ustr::from("nautilus_trader::adapters::binance"),
            LevelFilter::Warn,
        ),
        (Ustr::from("nautilus_trader::common"), LevelFilter::Debug),
        (Ustr::from("nautilus_trader::core"), LevelFilter::Info),
        (Ustr::from("nautilus_trader::data"), LevelFilter::Info),
        (Ustr::from("nautilus_trader::execution"), LevelFilter::Debug),
        (Ustr::from("nautilus_trader::model"), LevelFilter::Info),
        (Ustr::from("nautilus_trader::portfolio"), LevelFilter::Debug),
    ];
    module_filters.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));

    let mut group = c.benchmark_group("logging/filter");

    // Measures the empty filter path with no module/component filters configured.
    // This should stay close to a simple branch cost for default logging configs.
    group.bench_function("no_filters", |b| {
        let modules = Vec::new();
        let components = AHashMap::new();

        b.iter(|| {
            black_box(should_filter_log(
                black_box(&component),
                black_box(Level::Info),
                black_box(&modules),
                black_box(&components),
                black_box(false),
            ))
        });
    });

    // Measures longest-prefix scanning across 8 module filters.
    // This tells us whether O(n) prefix matching deserves a more complex data structure.
    group.bench_function("module_prefix_8_filters", |b| {
        let components = AHashMap::new();

        b.iter(|| {
            black_box(should_filter_log(
                black_box(&component),
                black_box(Level::Info),
                black_box(&module_filters),
                black_box(&components),
                black_box(false),
            ))
        });
    });

    // Measures an exact component-filter hit through AHashMap.
    // This represents producer-side filtering when log levels are tuned by component name.
    group.bench_function("component_filter_hit", |b| {
        let modules = Vec::new();

        b.iter(|| {
            black_box(should_filter_log(
                black_box(&component),
                black_box(Level::Info),
                black_box(&modules),
                black_box(&component_levels),
                black_box(false),
            ))
        });
    });

    group.finish();
}

fn bench_file_writer(c: &mut Criterion) {
    let mut group = c.benchmark_group("logging/file_writer");

    // Measures file-writer cost for a clean plain line.
    // The key question is whether the sanitizer fast path avoids regex work and String allocation.
    group.bench_function("plain_clean_line", |b| {
        let (_dir, mut writer) = make_file_writer("plain-clean-line");

        b.iter(|| writer.write(black_box(CLEAN_FILE_LINE)));
    });

    // Measures file-writer cost for a line containing ANSI escapes.
    // This verifies the sanitizer slow path still cleans colored output without major regressions.
    group.bench_function("plain_ansi_line", |b| {
        let (_dir, mut writer) = make_file_writer("plain-ansi-line");

        b.iter(|| writer.write(black_box(ANSI_FILE_LINE)));
    });

    group.finish();
}

fn bench_file_flush(c: &mut Criterion) {
    let mut group = c.benchmark_group("logging/file_flush");

    // Measures buffered file flush when durability is left to the OS page cache.
    // This is the candidate replacement cost if `FileWriter::flush` ever drops `sync_all`.
    group.bench_function("bufwriter_flush_after_write", |b| {
        let file = tempfile::tempfile().expect("failed to create benchmark temp file");
        let mut writer = BufWriter::new(file);

        b.iter(|| {
            writer
                .write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                .expect("buffered file write should not fail");
            black_box(writer.flush().expect("buffered file flush should not fail"));
        });
    });

    // Measures the current durability semantics: flush userspace buffering, then fsync.
    // This isolates the cost that regular `Flush` events currently pay through `sync_all`.
    group.bench_function("bufwriter_flush_sync_all_after_write", |b| {
        let file = tempfile::tempfile().expect("failed to create benchmark temp file");
        let mut writer = BufWriter::new(file);

        b.iter(|| {
            writer
                .write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                .expect("buffered file write should not fail");
            writer.flush().expect("buffered file flush should not fail");
            black_box(
                writer
                    .get_ref()
                    .sync_all()
                    .expect("file sync should not fail"),
            );
        });
    });

    // Measures the production `FileWriter::flush` path after a normal log write.
    // Current production semantics include `sync_all`, so this should stay in the ms range.
    group.bench_function("file_writer_flush_after_write", |b| {
        let (_dir, mut writer) = make_file_writer("flush-after-write");

        b.iter(|| {
            writer.write(black_box(CLEAN_FILE_LINE));
            writer.flush();
        });
    });

    group.finish();
}

fn bench_std_stream_proxy(c: &mut Criterion) {
    let mut group = c.benchmark_group("logging/std_stream");

    // Measures the direct Write lower bound without syscalls.
    // `io::sink()` estimates the minimum overhead of the benchmark loop and write_all call.
    group.bench_function("direct_sink_line", |b| {
        let mut writer = io::sink();

        b.iter(|| {
            black_box(
                writer
                    .write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                    .expect("sink write should not fail"),
            );
        });
    });

    // Measures BufWriter's fixed overhead without syscalls.
    // If this is slower than direct sink, BufWriter can only win by reducing lower-level writes.
    group.bench_function("bufwriter_sink_line", |b| {
        let mut writer = BufWriter::new(io::sink());

        b.iter(|| {
            black_box(
                writer
                    .write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                    .expect("sink write should not fail"),
            );
        });
    });

    // Uses a tempfile as a syscall-like proxy for direct stdout/stderr writes.
    // This is not an exact terminal benchmark, but it shows the order of magnitude for kernel writes.
    group.bench_function("direct_temp_file_line", |b| {
        let mut file = tempfile::tempfile().expect("failed to create benchmark temp file");

        b.iter(|| {
            black_box(
                file.write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                    .expect("file write should not fail"),
            );
        });
    });

    // Measures direct writer cost when every line is flushed.
    // This models conservative stderr/error semantics where each line should be visible immediately.
    group.bench_function("direct_temp_file_flush_each_line", |b| {
        let mut file = tempfile::tempfile().expect("failed to create benchmark temp file");

        b.iter(|| {
            file.write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                .expect("file write should not fail");
            black_box(file.flush().expect("file flush should not fail"));
        });
    });

    // Measures the benefit of BufWriter when flush can be delayed.
    // This informs the "can stdout be buffered?" tradeoff, but does not directly represent stderr.
    group.bench_function("bufwriter_temp_file_line", |b| {
        let file = tempfile::tempfile().expect("failed to create benchmark temp file");
        let mut writer = BufWriter::new(file);

        b.iter(|| {
            black_box(
                writer
                    .write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                    .expect("buffered file write should not fail"),
            );
        });
    });

    // Measures whether BufWriter still helps when every line is flushed.
    // If this matches direct flush, stderr/error should not be buffered only for performance.
    group.bench_function("bufwriter_temp_file_flush_each_line", |b| {
        let file = tempfile::tempfile().expect("failed to create benchmark temp file");
        let mut writer = BufWriter::new(file);

        b.iter(|| {
            writer
                .write_all(black_box(CLEAN_FILE_LINE).as_bytes())
                .expect("buffered file write should not fail");
            black_box(writer.flush().expect("buffered file flush should not fail"));
        });
    });

    group.finish();
}

fn bench_channel_send(c: &mut Criterion) {
    let no_fields = make_log_line_without_fields();
    let with_two_fields = make_log_line_with_two_fields();
    let with_fields = make_log_line_with_fields();
    let (tx, rx) = std::sync::mpsc::channel::<LogEvent>();
    let drain = thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            black_box(event);
        }
    });

    let mut group = c.benchmark_group("logging/channel");

    // Measures by-value channel send cost for LogEvent without extra fields.
    // This is the most common producer-side payload and validates LogLine size reductions.
    group.bench_function("send_log_event_no_fields", |b| {
        b.iter_batched(
            || no_fields.clone(),
            |line| black_box(tx.send(LogEvent::Log(line)).expect("receiver should drain")),
            BatchSize::SmallInput,
        );
    });

    // Measures by-value channel send cost with 4 extra fields.
    // This exposes how structured-field storage affects channel copy/drop paths.
    group.bench_function("send_log_event_4_fields", |b| {
        b.iter_batched(
            || with_fields.clone(),
            |line| black_box(tx.send(LogEvent::Log(line)).expect("receiver should drain")),
            BatchSize::SmallInput,
        );
    });

    // Measures a middle channel payload with 2 extra fields.
    // This keeps LogFields decisions from being based only on extreme payloads.
    group.bench_function("send_log_event_2_fields", |b| {
        b.iter_batched(
            || with_two_fields.clone(),
            |line| black_box(tx.send(LogEvent::Log(line)).expect("receiver should drain")),
            BatchSize::SmallInput,
        );
    });

    group.finish();
    drop(tx);
    drain.join().expect("drain thread should exit");
}

fn bench_producer_filter(c: &mut Criterion) {
    init_filtered_global_logger();

    let mut group = c.benchmark_group("logging/producer");

    // Measures the filtered producer path with a KV component.
    // This covers FieldCollector, component parsing, producer-side filtering, and skipped format/send.
    group.bench_function("filtered_info_component", |b| {
        b.iter(|| {
            log::info!(
                component = FILTERED_COMPONENT;
                "filtered benchmark message {}", black_box(42)
            );
        });
    });

    // Measures filtered producer cost when non-reserved KV fields are present.
    // This catches regressions where filtered logs still stringify structured fields before skip.
    group.bench_function("filtered_info_component_with_fields", |b| {
        b.iter(|| {
            log::info!(
                component = FILTERED_COMPONENT,
                venue = "BINANCE",
                latency_ns = black_box(42_u64);
                "filtered benchmark message {}", black_box(42)
            );
        });
    });

    // Measures the filtered producer path that relies on metadata target fallback.
    // This isolates `record.metadata().target()` and repeated-Ustr cache behavior.
    group.bench_function("filtered_info_target", |b| {
        b.iter(|| {
            log::info!(
                target: FILTERED_TARGET,
                "filtered benchmark message {}", black_box(42)
            );
        });
    });

    // Measures metadata-target filtering with extra KV fields.
    // This verifies target-filtered structured logs skip field String allocation before drop.
    group.bench_function("filtered_info_target_with_fields", |b| {
        b.iter(|| {
            log::info!(
                target: FILTERED_TARGET,
                venue = "BINANCE",
                latency_ns = black_box(42_u64);
                "filtered benchmark message {}", black_box(42)
            );
        });
    });

    // Measures the project-level `logger::log(...)` wrapper.
    // This API passes a Ustr component and color KV, matching common Nautilus internal calls.
    group.bench_function("filtered_info_logger_api", |b| {
        let component = Ustr::from(FILTERED_COMPONENT);

        b.iter(|| {
            nautilus_common::logging::logger::log(
                LogLevel::Info,
                LogColor::Normal,
                black_box(component),
                black_box("filtered benchmark message"),
            );
        });
    });

    group.finish();
}

fn bench_producer_direct(c: &mut Criterion) {
    let no_filter_config =
        LoggerConfig::from_spec("stdout=Info;fileout=Off").expect("valid benchmark logger config");
    let filter_active_config =
        LoggerConfig::from_spec("stdout=Info;fileout=Off;BenchFiltered=Error")
            .expect("valid benchmark logger config");
    let (no_filter_logger, no_filter_drain) = make_direct_logger(no_filter_config);
    let (filter_active_logger, filter_active_drain) = make_direct_logger(filter_active_config);

    let mut group = c.benchmark_group("logging/producer_direct");

    // Measures the real no-filter `Logger::log` path without the global logger singleton.
    // The local channel is drained, so this captures producer construction and send cost only.
    group.bench_function("no_filter_info_target_no_fields", |b| {
        b.iter(|| {
            log_direct_no_fields(
                black_box(&no_filter_logger),
                black_box(COMPONENT),
                black_box(42),
            );
        });
    });

    // Measures repeated-Ustr cache pressure when one thread rotates over more targets than the
    // 8-slot TLS cache can hold. This bounds the miss-heavy producer fallback path.
    group.bench_function("no_filter_info_12_targets_no_fields", |b| {
        let mut target_index = 0_usize;

        b.iter(|| {
            let target = COMPONENT_TARGETS_12[target_index];
            target_index += 1;
            if target_index == COMPONENT_TARGETS_12.len() {
                target_index = 0;
            }

            log_direct_no_fields(
                black_box(&no_filter_logger),
                black_box(target),
                black_box(42),
            );
        });
    });

    // Measures no-filter structured logs through the one-visit FieldCollector path.
    // This is the production path when module/component filters are not configured.
    group.bench_function("no_filter_info_component_with_fields", |b| {
        b.iter(|| {
            log_direct_with_fields(
                black_box(&no_filter_logger),
                black_box(COMPONENT),
                black_box(COMPONENT),
                black_box(42),
            );
        });
    });

    // Measures filter-active logs that pass the filter and therefore pay the two-visitor path.
    // Compare against the no-filter structured case to bound E010's pass-through overhead.
    group.bench_function("filter_active_pass_info_component_with_fields", |b| {
        b.iter(|| {
            log_direct_with_fields(
                black_box(&filter_active_logger),
                black_box(COMPONENT),
                black_box(COMPONENT),
                black_box(42),
            );
        });
    });

    group.finish();

    drop(no_filter_logger);
    drop(filter_active_logger);
    no_filter_drain
        .join()
        .expect("no-filter drain thread should exit");
    filter_active_drain
        .join()
        .expect("filter-active drain thread should exit");
}

fn bench_ustr(c: &mut Criterion) {
    let mut group = c.benchmark_group("logging/ustr");

    // Measures repeated `Ustr::from(&'static str)` in isolation.
    // This helps decide whether the repeated-Ustr TLS cache belongs on the producer hot path.
    group.bench_function("from_static_target_repeat", |b| {
        b.iter(|| black_box(Ustr::from(black_box(COMPONENT))));
    });

    // Measures component-value stringification as an allocation-cost proxy.
    // Compare this with producer component benches to separate KV `to_string()` from Ustr interning.
    group.bench_function("component_value_to_string", |b| {
        b.iter(|| black_box(black_box(FILTERED_COMPONENT).to_string()));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_log_line_formatting,
    bench_filtering,
    bench_file_writer,
    bench_file_flush,
    bench_std_stream_proxy,
    bench_channel_send,
    bench_producer_filter,
    bench_producer_direct,
    bench_ustr
);
criterion_main!(benches);
