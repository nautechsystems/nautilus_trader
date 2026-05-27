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

use std::{
    cell::RefCell,
    fmt::{Display, Write as _},
    sync::{Mutex, OnceLock, atomic::Ordering, mpsc::SendError},
};

use ahash::AHashMap;
use indexmap::IndexMap;
use log::{
    Level, LevelFilter, Log, STATIC_MAX_LEVEL,
    kv::{ToValue, Value},
    set_boxed_logger, set_max_level,
};
use nautilus_core::{
    UUID4, UnixNanos,
    datetime::unix_nanos_to_iso8601,
    time::{get_atomic_clock_realtime, get_atomic_clock_static},
};
use nautilus_model::identifiers::TraderId;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use smallvec::SmallVec;
use ustr::Ustr;

pub use super::config::LoggerConfig;
use super::{LOGGING_BYPASSED, LOGGING_GUARDS_ACTIVE, LOGGING_INITIALIZED, LOGGING_REALTIME};
#[cfg(not(all(feature = "simulation", madsim)))]
use crate::logging::writer::{FileWriter, LogWriter, StderrWriter, StdoutWriter};
use crate::{
    enums::{LogColor, LogLevel},
    logging::writer::FileWriterConfig,
};

#[cfg(not(all(feature = "simulation", madsim)))]
const LOGGING: &str = "logging";
const KV_COLOR: &str = "color";
const KV_COMPONENT: &str = "component";
const LOG_FIELDS_INLINE_CAP: usize = 0;
const MAX_LEVEL_DISPLAY_LEN: usize = "ERROR".len();
const ANSI_BOLD_LEN: usize = "\x1b[1m".len();
const ANSI_RESET_LEN: usize = "\x1b[0m".len();
const PLAIN_FORMAT_OVERHEAD: usize = " [".len() + "] ".len() + ".".len() + ": ".len() + "\n".len();
const COLORED_FORMAT_OVERHEAD: usize = ANSI_BOLD_LEN
    + ANSI_RESET_LEN
    + " ".len()
    + "[".len()
    + "] ".len()
    + ".".len()
    + ": ".len()
    + ANSI_RESET_LEN
    + "\n".len();
const REPEATED_USTR_CACHE_CAP: usize = 8;

thread_local! {
    static REPEATED_USTR_CACHE: RefCell<RepeatedUstrCache> =
        const { RefCell::new(RepeatedUstrCache::new()) };
}

#[derive(Clone, Copy)]
struct RepeatedUstrCacheEntry {
    ptr: usize,
    len: usize,
    value: Ustr,
}

#[derive(Clone, Copy)]
struct RepeatedUstrCache {
    entries: [Option<RepeatedUstrCacheEntry>; REPEATED_USTR_CACHE_CAP],
    next: usize,
}

impl RepeatedUstrCache {
    const fn new() -> Self {
        Self {
            entries: [None; REPEATED_USTR_CACHE_CAP],
            next: 0,
        }
    }
}

/// Storage for structured log fields.
/// Inline capacity is intentionally zero to keep the producer-side `LogLine` payload small.
pub type LogFields = SmallVec<[(Ustr, String); LOG_FIELDS_INLINE_CAP]>;

/// Global log sender which allows multiple log guards per process.
static LOGGER_TX: OnceLock<std::sync::mpsc::Sender<LogEvent>> = OnceLock::new();

/// Global handle to the logging thread - only one thread exists per process.
static LOGGER_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// Producer-side filtering policy derived from [`LoggerConfig`].
#[derive(Debug, Clone)]
struct FilterPolicy {
    /// Module filters pre-sorted by descending path length for longest-prefix lookup.
    modules_by_longest_prefix: Vec<(Ustr, LevelFilter)>,
    /// Per-component log level overrides.
    components: AHashMap<Ustr, LevelFilter>,
    /// Whether logs without an explicit component/module filter should be skipped.
    components_only: bool,
}

impl FilterPolicy {
    fn from_config(config: &LoggerConfig) -> Option<Self> {
        let modules_by_longest_prefix = sorted_module_filters_from_map(&config.module_level);
        if !config.log_components_only
            && modules_by_longest_prefix.is_empty()
            && config.component_level.is_empty()
        {
            return None;
        }

        Some(Self {
            modules_by_longest_prefix,
            components: config.component_level.clone(),
            components_only: config.log_components_only,
        })
    }

    fn should_skip(&self, component: &Ustr, level: Level) -> bool {
        should_filter_log_inner(
            component,
            level,
            &self.modules_by_longest_prefix,
            &self.components,
            self.components_only,
        )
    }
}

/// A high-performance logger utilizing a MPSC channel under the hood.
///
/// A logger is initialized with a [`LoggerConfig`] to set up different logging levels for
/// stdout, file, and components. The logger spawns a thread that listens for [`LogEvent`]s
/// sent via an MPSC channel.
#[derive(Debug)]
pub struct Logger {
    /// Initialization snapshot for logging levels and behavior.
    ///
    /// Producer filters are derived into `filter_policy` at initialization; mutating this field
    /// after registration does not reload component/module filters.
    pub config: LoggerConfig,
    /// Producer-side component/module filtering policy.
    filter_policy: Option<FilterPolicy>,
    /// Transmitter for sending log events to the 'logging' thread.
    tx: std::sync::mpsc::Sender<LogEvent>,
}

/// Represents a type of log event.
#[derive(Debug)]
pub enum LogEvent {
    /// A log line event.
    Log(LogLine),
    /// A command to flush all logger buffers.
    Flush,
    /// A command to close the logger.
    Close,
}

/// Represents a log event which includes a message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogLine {
    /// The timestamp for the event.
    pub timestamp: UnixNanos,
    /// The log level for the event.
    pub level: Level,
    /// The color for the log message content.
    pub color: LogColor,
    /// The Nautilus system component the log event originated from.
    pub component: Ustr,
    /// The log message content.
    pub message: String,
    /// Arbitrary structured key-value fields attached to this log event.
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub fields: LogFields,
}

impl Display for LogLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.level, self.component, self.message)?;
        for (k, v) in &self.fields {
            write!(f, " {k}={v}")?;
        }
        Ok(())
    }
}

/// A wrapper around a log line that provides formatted and cached representations.
///
/// This struct contains a log line and provides various formatted versions
/// of it, such as plain string, colored string, and JSON. It also caches the
/// results for repeated calls, optimizing performance when the same message
/// needs to be logged multiple times in different formats.
#[derive(Clone, Debug)]
pub struct LogLineWrapper {
    /// The underlying log line that contains the log data.
    line: LogLine,
    /// Cached plain string representation of the log line.
    cache: Option<String>,
    /// Cached colored string representation of the log line.
    colored: Option<String>,
    /// The ID of the trader associated with this log event.
    trader_id: Ustr,
}

impl LogLineWrapper {
    /// Creates a new [`LogLineWrapper`] instance.
    #[must_use]
    pub const fn new(line: LogLine, trader_id: Ustr) -> Self {
        Self {
            line,
            cache: None,
            colored: None,
            trader_id,
        }
    }

    /// Returns the plain log message string, caching the result.
    ///
    /// This method constructs the log line format and caches it for repeated calls. Useful when the
    /// same log message needs to be printed multiple times.
    pub fn get_string(&mut self) -> &str {
        self.cache.get_or_insert_with(|| {
            let timestamp = unix_nanos_to_iso8601(self.line.timestamp);
            let mut s = String::with_capacity(plain_log_line_capacity(
                &timestamp,
                self.trader_id,
                &self.line,
            ));

            write!(
                s,
                "{} [{}] {}.{}: {}",
                timestamp,
                self.line.level,
                self.trader_id,
                &self.line.component,
                &self.line.message,
            )
            .expect("writing to String should not fail");

            for (k, v) in &self.line.fields {
                s.push(' ');
                s.push_str(k);
                s.push('=');
                s.push_str(v);
            }
            s.push('\n');
            s
        })
    }

    /// Returns the colored log message string, caching the result.
    ///
    /// This method constructs the colored log line format and caches the result
    /// for repeated calls, providing the message with ANSI color codes if the
    /// logger is configured to use colors.
    pub fn get_colored(&mut self) -> &str {
        self.colored.get_or_insert_with(|| {
            let timestamp = unix_nanos_to_iso8601(self.line.timestamp);
            let color_ansi = self.line.color.as_ansi();
            let mut s = String::with_capacity(colored_log_line_capacity(
                &timestamp,
                color_ansi,
                self.trader_id,
                &self.line,
            ));

            write!(
                s,
                "\x1b[1m{}\x1b[0m {}[{}] {}.{}: {}",
                timestamp,
                color_ansi,
                self.line.level,
                self.trader_id,
                &self.line.component,
                &self.line.message,
            )
            .expect("writing to String should not fail");

            for (k, v) in &self.line.fields {
                s.push(' ');
                s.push_str(k);
                s.push('=');
                s.push_str(v);
            }
            s.push_str("\x1b[0m\n");
            s
        })
    }

    /// Returns the log message as a JSON string.
    ///
    /// This method serializes the log line and its associated metadata
    /// (timestamp, trader ID, etc.) into a JSON string format. This is useful
    /// for structured logging or when logs need to be stored in a JSON format.
    /// # Panics
    ///
    /// Panics if serialization of the log event to JSON fails.
    #[must_use]
    pub fn get_json(&self) -> String {
        let mut json_string =
            serde_json::to_string(&self).expect("Error serializing log event to string");
        json_string.push('\n');
        json_string
    }
}

fn formatted_fields_len(fields: &LogFields) -> usize {
    fields.iter().map(|(k, v)| 2 + k.len() + v.len()).sum()
}

fn log_line_capacity(
    timestamp: &str,
    trader_id: Ustr,
    line: &LogLine,
    overhead: usize,
    ansi_extra_len: usize,
) -> usize {
    timestamp.len()
        + overhead
        + ansi_extra_len
        + MAX_LEVEL_DISPLAY_LEN
        + trader_id.len()
        + line.component.len()
        + line.message.len()
        + formatted_fields_len(&line.fields)
}

fn plain_log_line_capacity(timestamp: &str, trader_id: Ustr, line: &LogLine) -> usize {
    log_line_capacity(timestamp, trader_id, line, PLAIN_FORMAT_OVERHEAD, 0)
}

fn colored_log_line_capacity(
    timestamp: &str,
    color_ansi: &str,
    trader_id: Ustr,
    line: &LogLine,
) -> usize {
    log_line_capacity(
        timestamp,
        trader_id,
        line,
        COLORED_FORMAT_OVERHEAD,
        color_ansi.len(),
    )
}

impl Serialize for LogLineWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if has_duplicate_json_field(&self.line.fields) {
            return serialize_log_line_with_indexmap(self, serializer);
        }

        let timestamp = unix_nanos_to_iso8601(self.line.timestamp);
        let mut map = serializer.serialize_map(None)?;

        map.serialize_entry("timestamp", &timestamp)?;
        map.serialize_entry("trader_id", self.trader_id.as_str())?;
        map.serialize_entry("level", &DisplayAsString(&self.line.level))?;
        map.serialize_entry("color", &DisplayAsString(&self.line.color))?;
        map.serialize_entry("component", self.line.component.as_str())?;
        map.serialize_entry("message", &self.line.message)?;

        for (k, v) in &self.line.fields {
            let key = k.as_str();
            if !is_reserved_json_key(key) {
                map.serialize_entry(key, v)?;
            }
        }

        map.end()
    }
}

struct DisplayAsString<'a, T: ?Sized>(&'a T);

impl<T> Serialize for DisplayAsString<'_, T>
where
    T: Display + ?Sized,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self.0)
    }
}

fn is_reserved_json_key(key: &str) -> bool {
    matches!(
        key,
        "timestamp" | "trader_id" | "level" | "color" | "component" | "message"
    )
}

fn has_duplicate_json_field(fields: &LogFields) -> bool {
    if fields.is_empty() {
        return false;
    }

    for (idx, (key, _)) in fields.iter().enumerate() {
        let key = key.as_str();
        if is_reserved_json_key(key) {
            continue;
        }

        if fields
            .iter()
            .take(idx)
            .any(|(prev, _)| !is_reserved_json_key(prev.as_str()) && prev.as_str() == key)
        {
            return true;
        }
    }

    false
}

fn serialize_log_line_with_indexmap<S>(
    wrapper: &LogLineWrapper,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut json_obj = IndexMap::new();
    let timestamp = unix_nanos_to_iso8601(wrapper.line.timestamp);
    json_obj.insert("timestamp".to_string(), timestamp);
    json_obj.insert("trader_id".to_string(), wrapper.trader_id.to_string());
    json_obj.insert("level".to_string(), wrapper.line.level.to_string());
    json_obj.insert("color".to_string(), wrapper.line.color.to_string());
    json_obj.insert("component".to_string(), wrapper.line.component.to_string());
    json_obj.insert("message".to_string(), wrapper.line.message.clone());
    for (k, v) in &wrapper.line.fields {
        let key = k.as_str();
        if !is_reserved_json_key(key) {
            json_obj.insert(k.to_string(), v.clone());
        }
    }

    json_obj.serialize(serializer)
}

fn sorted_module_filters_from_map(
    module_level: &AHashMap<Ustr, LevelFilter>,
) -> Vec<(Ustr, LevelFilter)> {
    let mut filters: Vec<_> = module_level
        .iter()
        .map(|(path, level)| (*path, *level))
        .collect();
    filters.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    filters
}

fn current_log_timestamp() -> UnixNanos {
    if LOGGING_REALTIME.load(Ordering::Relaxed) {
        get_atomic_clock_realtime().get_time_ns()
    } else {
        get_atomic_clock_static().get_time_ns()
    }
}

fn intern_repeated(value: &str) -> Ustr {
    REPEATED_USTR_CACHE.with(|cache| {
        let mut cache_state = cache.borrow_mut();
        let ptr = value.as_ptr() as usize;
        let len = value.len();

        // Targets and components are usually repeated static strings; the content check keeps
        // dynamic RecordBuilder targets correct if an allocator reuses the same pointer.
        for entry in cache_state.entries.iter().flatten() {
            if entry.ptr == ptr && entry.len == len && entry.value.as_str() == value {
                return entry.value;
            }
        }

        let interned = Ustr::from(value);
        let insert_idx = cache_state.next;
        cache_state.entries[insert_idx] = Some(RepeatedUstrCacheEntry {
            ptr,
            len,
            value: interned,
        });
        cache_state.next = (insert_idx + 1) % REPEATED_USTR_CACHE_CAP;
        interned
    })
}

fn intern_component_value(value: &log::kv::Value<'_>) -> Ustr {
    match value.to_borrowed_str() {
        Some(component) => intern_repeated(component),
        None => Ustr::from(&value.to_string()),
    }
}

struct ComponentProbe {
    component: Option<Ustr>,
}

impl ComponentProbe {
    const fn new() -> Self {
        Self { component: None }
    }
}

impl<'kvs> log::kv::VisitSource<'kvs> for ComponentProbe {
    fn visit_pair(
        &mut self,
        key: log::kv::Key<'kvs>,
        value: log::kv::Value<'kvs>,
    ) -> Result<(), log::kv::Error> {
        if key.as_str() == KV_COMPONENT {
            self.component = Some(intern_component_value(&value));
        }
        Ok(())
    }
}

struct PayloadCollector {
    color: Option<LogColor>,
    fields: LogFields,
}

impl PayloadCollector {
    fn new() -> Self {
        Self {
            color: None,
            fields: SmallVec::new(),
        }
    }
}

impl<'kvs> log::kv::VisitSource<'kvs> for PayloadCollector {
    fn visit_pair(
        &mut self,
        key: log::kv::Key<'kvs>,
        value: log::kv::Value<'kvs>,
    ) -> Result<(), log::kv::Error> {
        match key.as_str() {
            KV_COLOR => {
                self.color = value.to_u64().map(|v| (v as u8).into());
            }
            KV_COMPONENT => {}
            _ => {
                self.fields
                    .push((Ustr::from(key.as_str()), value.to_string()));
            }
        }
        Ok(())
    }
}

struct FieldCollector {
    color: Option<LogColor>,
    component: Option<Ustr>,
    fields: LogFields,
}

impl FieldCollector {
    fn new() -> Self {
        Self {
            color: None,
            component: None,
            fields: SmallVec::new(),
        }
    }
}

impl<'kvs> log::kv::VisitSource<'kvs> for FieldCollector {
    fn visit_pair(
        &mut self,
        key: log::kv::Key<'kvs>,
        value: log::kv::Value<'kvs>,
    ) -> Result<(), log::kv::Error> {
        match key.as_str() {
            KV_COLOR => {
                self.color = value.to_u64().map(|v| (v as u8).into());
            }
            KV_COMPONENT => {
                self.component = Some(intern_component_value(&value));
            }
            _ => {
                self.fields
                    .push((Ustr::from(key.as_str()), value.to_string()));
            }
        }
        Ok(())
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !LOGGING_BYPASSED.load(Ordering::Relaxed)
            && (metadata.level() == Level::Error
                || metadata.level() <= self.config.stdout_level
                || metadata.level() <= self.config.fileout_level)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let level = record.level();

            if let Some(filter_policy) = &self.filter_policy {
                // Probe only the component before filtering. Skipped logs should not pay for
                // timestamps, message formatting, or non-reserved structured-field strings.
                let mut probe = ComponentProbe::new();
                let _ = record.key_values().visit(&mut probe);
                let component = probe
                    .component
                    .unwrap_or_else(|| intern_repeated(record.metadata().target()));

                if filter_policy.should_skip(&component, level) {
                    return;
                }

                let timestamp = current_log_timestamp();
                let mut collector = PayloadCollector::new();
                let _ = record.key_values().visit(&mut collector);
                let color = collector.color.unwrap_or_else(|| level.into());

                self.send_log_line(LogLine {
                    timestamp,
                    level,
                    color,
                    component,
                    message: format!("{}", record.args()),
                    fields: collector.fields,
                });
                return;
            }

            // With no component/module filters configured, keep the producer path to one KV visit.
            let timestamp = current_log_timestamp();
            let mut collector = FieldCollector::new();
            let _ = record.key_values().visit(&mut collector);
            let color = collector.color.unwrap_or_else(|| level.into());
            let component = collector
                .component
                .unwrap_or_else(|| intern_repeated(record.metadata().target()));

            let line = LogLine {
                timestamp,
                level,
                color,
                component,
                message: format!("{}", record.args()),
                fields: collector.fields,
            };

            self.send_log_line(line);
        }
    }

    fn flush(&self) {
        // Don't attempt to flush if we're already bypassed/shutdown
        if LOGGING_BYPASSED.load(Ordering::Relaxed) {
            return;
        }

        if let Err(e) = self.tx.send(LogEvent::Flush) {
            eprintln!("Error sending flush log event: {e}");
        }
    }
}

impl Logger {
    /// Creates a logger instance for direct benchmark harnesses.
    ///
    /// This bypasses the global `log::set_logger` singleton so benchmark code can compare
    /// multiple logger configurations in one process. It does not spawn a writer thread.
    #[doc(hidden)]
    #[must_use]
    pub fn new_for_benchmark(config: LoggerConfig, tx: std::sync::mpsc::Sender<LogEvent>) -> Self {
        let filter_policy = FilterPolicy::from_config(&config);

        Self {
            config,
            filter_policy,
            tx,
        }
    }

    fn send_log_line(&self, line: LogLine) {
        if let Err(SendError(LogEvent::Log(line))) = self.tx.send(LogEvent::Log(line)) {
            eprintln!("Error sending log event (receiver closed): {line}");
        }
    }

    /// Initializes the logger based on the `NAUTILUS_LOG` environment variable.
    ///
    /// # Errors
    ///
    /// Returns an error if reading the environment variable or parsing the configuration fails.
    pub fn init_with_env(
        trader_id: TraderId,
        instance_id: UUID4,
        file_config: FileWriterConfig,
    ) -> anyhow::Result<LogGuard> {
        let config = LoggerConfig::from_env()?;
        Self::init_with_config(trader_id, instance_id, config, file_config)
    }

    /// Initializes the logger with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the logger fails to register or initialize the background thread.
    pub fn init_with_config(
        trader_id: TraderId,
        instance_id: UUID4,
        config: LoggerConfig,
        file_config: FileWriterConfig,
    ) -> anyhow::Result<LogGuard> {
        // Fast path: already initialized
        if super::LOGGING_INITIALIZED.load(Ordering::SeqCst) {
            return LogGuard::new().ok_or_else(|| {
                anyhow::anyhow!("Logging already initialized but new guard could not be created")
            });
        }

        let (tx, rx) = std::sync::mpsc::channel::<LogEvent>();
        let filter_policy = FilterPolicy::from_config(&config);

        let logger_tx = tx.clone();
        let logger = Self {
            config: config.clone(),
            filter_policy,
            tx: logger_tx,
        };

        set_boxed_logger(Box::new(logger))?;

        // Store the sender globally so additional guards can be created
        if LOGGER_TX.set(tx).is_err() {
            debug_assert!(
                false,
                "LOGGER_TX already set - re-initialization not supported"
            );
        }

        if config.bypass_logging {
            super::logging_set_bypass();
        }

        let is_colored = config.is_colored;

        let print_config = config.print_config;
        if print_config {
            println!("STATIC_MAX_LEVEL={STATIC_MAX_LEVEL}");
            println!("Logger initialized with {config:?} {file_config:?}");
        }

        #[cfg(not(all(feature = "simulation", madsim)))]
        {
            let handle = std::thread::Builder::new()
                .name(LOGGING.to_string())
                .spawn(move || {
                    Self::handle_messages(
                        trader_id.to_string(),
                        instance_id.to_string(),
                        config,
                        file_config,
                        rx,
                    );
                })?;

            // Store the handle globally
            if let Ok(mut handle_guard) = LOGGER_HANDLE.lock() {
                debug_assert!(
                    handle_guard.is_none(),
                    "LOGGER_HANDLE already set - re-initialization not supported"
                );
                *handle_guard = Some(handle);
            }
        }

        #[cfg(all(feature = "simulation", madsim))]
        {
            // Under simulation, the background writer thread would escape the
            // madsim scheduler. Drop the receiver so the channel closes cleanly
            // and force the bypass flag so subsequent log calls no-op without
            // SendError noise.
            let _ = (trader_id, instance_id, config, file_config, rx);
            super::logging_set_bypass();
        }

        let max_level = log::LevelFilter::Trace;
        set_max_level(max_level);

        if print_config {
            println!("Logger set as `log` implementation with max level {max_level}");
        }

        super::LOGGING_INITIALIZED.store(true, Ordering::SeqCst);
        super::LOGGING_COLORED.store(is_colored, Ordering::SeqCst);

        LogGuard::new()
            .ok_or_else(|| anyhow::anyhow!("Failed to create LogGuard from global sender"))
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[expect(clippy::needless_pass_by_value)]
    fn handle_messages(
        trader_id: String,
        instance_id: String,
        config: LoggerConfig,
        file_config: FileWriterConfig,
        rx: std::sync::mpsc::Receiver<LogEvent>,
    ) {
        let LoggerConfig {
            stdout_level,
            fileout_level,
            component_level: _,
            module_level: _,
            log_components_only: _,
            is_colored,
            print_config: _,
            use_tracing: _,
            bypass_logging: _,
            file_config: _,
            clear_log_file,
        } = config;

        let trader_id_cache = Ustr::from(&trader_id);

        // Set up std I/O buffers
        let mut stdout_writer = StdoutWriter::new(stdout_level, is_colored);
        let mut stderr_writer = StderrWriter::new(is_colored);

        // Conditionally create file writer based on fileout_level
        let mut file_writer_opt = if fileout_level == LevelFilter::Off {
            None
        } else {
            FileWriter::new(
                trader_id,
                instance_id,
                file_config,
                fileout_level,
                clear_log_file,
            )
        };

        let process_event = |event: LogEvent,
                             stdout_writer: &mut StdoutWriter,
                             stderr_writer: &mut StderrWriter,
                             file_writer_opt: &mut Option<FileWriter>| {
            match event {
                LogEvent::Log(line) => {
                    let mut wrapper = LogLineWrapper::new(line, trader_id_cache);

                    if stderr_writer.enabled(&wrapper.line) {
                        if is_colored {
                            stderr_writer.write(wrapper.get_colored());
                        } else {
                            stderr_writer.write(wrapper.get_string());
                        }
                    }

                    if stdout_writer.enabled(&wrapper.line) {
                        if is_colored {
                            stdout_writer.write(wrapper.get_colored());
                        } else {
                            stdout_writer.write(wrapper.get_string());
                        }
                    }

                    if let Some(file_writer) = file_writer_opt
                        && file_writer.enabled(&wrapper.line)
                    {
                        if file_writer.json_format {
                            file_writer.write(&wrapper.get_json());
                        } else {
                            file_writer.write(wrapper.get_string());
                        }
                    }
                }
                LogEvent::Flush => {
                    stdout_writer.flush();
                    stderr_writer.flush();

                    if let Some(file_writer) = file_writer_opt {
                        file_writer.flush();
                    }
                }
                LogEvent::Close => {
                    // Close handled in the main loop; ignore here.
                }
            }
        };

        // Continue to receive and handle log events until channel is hung up
        while let Ok(event) = rx.recv() {
            match event {
                LogEvent::Log(_) | LogEvent::Flush => process_event(
                    event,
                    &mut stdout_writer,
                    &mut stderr_writer,
                    &mut file_writer_opt,
                ),
                LogEvent::Close => {
                    // First flush what's been written so far
                    stdout_writer.flush();
                    stderr_writer.flush();

                    if let Some(ref mut file_writer) = file_writer_opt {
                        file_writer.flush();
                    }

                    // Drain any remaining events that may have raced with shutdown
                    // This ensures logs enqueued just before/around shutdown aren't lost.
                    while let Ok(evt) = rx.try_recv() {
                        match evt {
                            LogEvent::Close => (), // ignore extra Close events
                            _ => process_event(
                                evt,
                                &mut stdout_writer,
                                &mut stderr_writer,
                                &mut file_writer_opt,
                            ),
                        }
                    }

                    // Final flush after draining
                    stdout_writer.flush();
                    stderr_writer.flush();

                    if let Some(ref mut file_writer) = file_writer_opt {
                        file_writer.flush();
                    }

                    break;
                }
            }
        }
    }
}

/// Determines if a log line should be filtered out based on module and component filters.
///
/// Returns `true` if the line should be skipped (filtered out), `false` if it should be logged.
///
/// The `module_filters_sorted` slice must be pre-sorted by descending path length so the
/// first `starts_with` match is the longest prefix.
#[must_use]
pub fn should_filter_log(
    component: &Ustr,
    line_level: log::Level,
    module_filters_sorted: &[(Ustr, LevelFilter)],
    component_level: &AHashMap<Ustr, LevelFilter>,
    log_components_only: bool,
) -> bool {
    should_filter_log_inner(
        component,
        line_level,
        module_filters_sorted,
        component_level,
        log_components_only,
    )
}

fn should_filter_log_inner(
    component: &Ustr,
    line_level: log::Level,
    module_filters_sorted: &[(Ustr, LevelFilter)],
    component_level: &AHashMap<Ustr, LevelFilter>,
    log_components_only: bool,
) -> bool {
    if module_filters_sorted.is_empty() && component_level.is_empty() {
        return log_components_only;
    }

    // Module filter: first match in sorted list is longest prefix
    let module_filter = module_filters_sorted
        .iter()
        .find(|(path, _)| component.starts_with(path.as_str()))
        .map(|(_, level)| *level);

    let component_filter = component_level.get(component).copied();

    if log_components_only && module_filter.is_none() && component_filter.is_none() {
        return true;
    }

    // Module filter takes precedence over component filter
    if let Some(filter_level) = module_filter.or(component_filter)
        && line_level > filter_level
    {
        return true;
    }

    false
}

/// Gracefully shuts down the logging subsystem.
///
/// Performs the same shutdown sequence as dropping the last `LogGuard`, but can be called
/// explicitly for deterministic shutdown timing (e.g., testing or Windows Python applications).
///
/// # Safety
///
/// Safe to call multiple times. Thread join is skipped if called from the logging thread.
pub(crate) fn shutdown_graceful() {
    // Prevent further logging
    LOGGING_BYPASSED.store(true, Ordering::SeqCst);
    log::set_max_level(log::LevelFilter::Off);

    // Signal Close if the sender exists
    if let Some(tx) = LOGGER_TX.get() {
        let _ = tx.send(LogEvent::Close);
    }

    if let Ok(mut handle_guard) = LOGGER_HANDLE.lock()
        && let Some(handle) = handle_guard.take()
        && handle.thread().id() != std::thread::current().id()
    {
        let _ = handle.join();
    }

    LOGGING_INITIALIZED.store(false, Ordering::SeqCst);
}

pub fn log<T: AsRef<str>>(level: LogLevel, color: LogColor, component: Ustr, message: T) {
    let color = Value::from(color as u8);

    match level {
        LogLevel::Off => {}
        LogLevel::Trace => {
            log::trace!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Debug => {
            log::debug!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Info => {
            log::info!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Warning => {
            log::warn!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
        LogLevel::Error => {
            log::error!(component = component.to_value(), color = color; "{}", message.as_ref());
        }
    }
}

/// A guard that manages the lifecycle of the logging subsystem.
///
/// `LogGuard` ensures the logging thread remains active while instances exist and properly
/// terminates when all guards are dropped. The system uses reference counting to track active
/// guards - when the last `LogGuard` is dropped, the logging thread is joined to ensure all
/// pending log messages are written before the process terminates.
///
/// # Reference Counting
///
/// The logging system maintains a global atomic counter of active `LogGuard` instances. This
/// ensures that:
/// - The logging thread remains active as long as at least one `LogGuard` exists.
/// - All log messages are properly flushed when intermediate guards are dropped.
/// - The logging thread is cleanly terminated and joined when the last guard is dropped.
///
/// # Shutdown Behavior
///
/// When the last guard is dropped, the logging thread is signaled to close, drains pending
/// messages, and is joined to ensure all logs are written before process termination.
///
/// **Python on Windows:** Non-deterministic GC order during interpreter shutdown can
/// occasionally prevent proper thread join, resulting in truncated logs.
///
/// # Limits
///
/// The system supports a maximum of 255 concurrent `LogGuard` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
#[derive(Debug)]
pub struct LogGuard {
    tx: std::sync::mpsc::Sender<LogEvent>,
}

impl LogGuard {
    /// Creates a new [`LogGuard`] instance from the global logger.
    ///
    /// Returns `None` if logging has not been initialized or the active `LogGuard`
    /// count would exceed 255.
    #[must_use]
    pub fn new() -> Option<Self> {
        let tx = LOGGER_TX.get()?;
        LOGGING_GUARDS_ACTIVE
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                if count == u8::MAX {
                    None
                } else {
                    Some(count + 1)
                }
            })
            .ok()?;

        Some(Self { tx: tx.clone() })
    }
}

impl Drop for LogGuard {
    /// Handles cleanup when a `LogGuard` is dropped.
    ///
    /// Sends `Flush` if other guards remain active, otherwise sends `Close`, joins the
    /// logging thread, and resets the subsystem state.
    fn drop(&mut self) {
        let previous_count = LOGGING_GUARDS_ACTIVE
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                assert!(count != 0, "LogGuard reference count underflow");
                Some(count - 1)
            })
            .expect("Failed to decrement LogGuard count");

        // Check if this was the last LogGuard - re-check after decrement to avoid race
        if previous_count == 1 && LOGGING_GUARDS_ACTIVE.load(Ordering::SeqCst) == 0 {
            // This is truly the last LogGuard, so we should close the logger and join the thread
            // to ensure all log messages are written before the process terminates.
            // Prevent any new log events from being accepted while shutting down.
            LOGGING_BYPASSED.store(true, Ordering::SeqCst);

            // Disable all log levels to reduce overhead on late calls
            log::set_max_level(log::LevelFilter::Off);

            // Ensure Close is delivered before joining (critical for shutdown)
            let _ = self.tx.send(LogEvent::Close);

            // Join the logging thread to ensure all pending logs are written
            if let Ok(mut handle_guard) = LOGGER_HANDLE.lock()
                && let Some(handle) = handle_guard.take()
            {
                // Avoid self-join deadlock
                if handle.thread().id() != std::thread::current().id() {
                    let _ = handle.join();
                }
            }

            // Reset LOGGING_INITIALIZED since the logging thread has terminated
            LOGGING_INITIALIZED.store(false, Ordering::SeqCst);
        } else {
            // Other LogGuards are still active, just flush our logs
            let _ = self.tx.send(LogEvent::Flush);
        }
    }
}

#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use log::LevelFilter;
    use nautilus_core::UUID4;
    use nautilus_model::identifiers::TraderId;
    use rstest::*;
    use serde_json::Value;
    use tempfile::tempdir;
    use ustr::Ustr;

    use super::*;
    use crate::enums::LogColor;

    #[rstest]
    fn log_message_serialization() {
        let log_message = LogLine {
            timestamp: UnixNanos::default(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("Portfolio"),
            message: "This is a log message".to_string(),
            fields: SmallVec::new(),
        };

        let serialized_json = serde_json::to_string(&log_message).unwrap();
        let deserialized_value: Value = serde_json::from_str(&serialized_json).unwrap();

        assert_eq!(deserialized_value["level"], "INFO");
        assert_eq!(deserialized_value["component"], "Portfolio");
        assert_eq!(deserialized_value["message"], "This is a log message");
    }

    #[rstest]
    fn log_config_parsing() {
        let config =
            LoggerConfig::from_spec("stdout=Info;is_colored;fileout=Debug;RiskEngine=Error")
                .unwrap();
        assert_eq!(
            config,
            LoggerConfig {
                stdout_level: LevelFilter::Info,
                fileout_level: LevelFilter::Debug,
                component_level: AHashMap::from_iter(vec![(
                    Ustr::from("RiskEngine"),
                    LevelFilter::Error
                )]),
                module_level: AHashMap::new(),
                log_components_only: false,
                is_colored: true,
                print_config: false,
                use_tracing: false,
                ..Default::default()
            }
        );
    }

    #[rstest]
    fn log_config_parsing2() {
        let config = LoggerConfig::from_spec("stdout=Warn;print_config;fileout=Error;").unwrap();
        assert_eq!(
            config,
            LoggerConfig {
                stdout_level: LevelFilter::Warn,
                fileout_level: LevelFilter::Error,
                component_level: AHashMap::new(),
                module_level: AHashMap::new(),
                log_components_only: false,
                is_colored: true,
                print_config: true,
                use_tracing: false,
                ..Default::default()
            }
        );
    }

    #[rstest]
    fn log_config_parsing_with_log_components_only() {
        let config =
            LoggerConfig::from_spec("stdout=Info;log_components_only;RiskEngine=Debug").unwrap();
        assert_eq!(
            config,
            LoggerConfig {
                stdout_level: LevelFilter::Info,
                fileout_level: LevelFilter::Off,
                component_level: AHashMap::from_iter(vec![(
                    Ustr::from("RiskEngine"),
                    LevelFilter::Debug
                )]),
                module_level: AHashMap::new(),
                log_components_only: true,
                is_colored: true,
                print_config: false,
                use_tracing: false,
                ..Default::default()
            }
        );
    }

    #[rstest]
    fn test_log_line_wrapper_plain_string() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("TestComponent"),
            message: "Test message".to_string(),
            fields: SmallVec::new(),
        };

        let mut wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-001"));
        let result = wrapper.get_string();

        assert!(result.contains("TRADER-001"));
        assert!(result.contains("TestComponent"));
        assert!(result.contains("Test message"));
        assert!(result.contains("[INFO]"));
        assert!(result.ends_with('\n'));
        // Should NOT contain ANSI codes
        assert!(!result.contains("\x1b["));
    }

    #[rstest]
    fn test_log_line_wrapper_colored_string() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Info,
            color: LogColor::Green,
            component: Ustr::from("TestComponent"),
            message: "Test message".to_string(),
            fields: SmallVec::new(),
        };

        let mut wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-001"));
        let result = wrapper.get_colored();

        assert!(result.contains("TRADER-001"));
        assert!(result.contains("TestComponent"));
        assert!(result.contains("Test message"));
        // Should contain ANSI codes
        assert!(result.contains("\x1b["));
        assert!(result.ends_with('\n'));
    }

    #[rstest]
    fn test_log_line_wrapper_json_output() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Warn,
            color: LogColor::Yellow,
            component: Ustr::from("RiskEngine"),
            message: "Warning message".to_string(),
            fields: SmallVec::new(),
        };

        let wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-002"));
        let json = wrapper.get_json();

        let parsed: Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["trader_id"], "TRADER-002");
        assert_eq!(parsed["component"], "RiskEngine");
        assert_eq!(parsed["message"], "Warning message");
        assert_eq!(parsed["level"], "WARN");
        assert_eq!(parsed["color"], "YELLOW");
    }

    #[rstest]
    fn test_log_line_wrapper_caches_string() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("Test"),
            message: "Cached".to_string(),
            fields: SmallVec::new(),
        };

        let mut wrapper = LogLineWrapper::new(line, Ustr::from("TRADER"));
        let first = wrapper.get_string().to_string();
        let second = wrapper.get_string().to_string();

        assert_eq!(first, second);
    }

    #[rstest]
    fn test_log_line_display() {
        let line = LogLine {
            timestamp: 0.into(),
            level: log::Level::Error,
            color: LogColor::Red,
            component: Ustr::from("Component"),
            message: "Error occurred".to_string(),
            fields: SmallVec::new(),
        };

        let display = format!("{line}");
        assert_eq!(display, "[ERROR] Component: Error occurred");
    }

    #[rstest]
    fn test_log_line_display_with_fields() {
        let line = LogLine {
            timestamp: 0.into(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("RiskEngine"),
            message: "Order filled".to_string(),
            fields: smallvec::smallvec![
                (Ustr::from("venue"), "BINANCE".to_string()),
                (Ustr::from("order_id"), "O-001".to_string()),
            ],
        };

        let display = format!("{line}");
        assert_eq!(
            display,
            "[INFO] RiskEngine: Order filled venue=BINANCE order_id=O-001"
        );
    }

    #[rstest]
    fn test_log_line_wrapper_plain_string_with_fields() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("DataEngine"),
            message: "Connected".to_string(),
            fields: smallvec::smallvec![
                (Ustr::from("venue"), "BINANCE".to_string()),
                (Ustr::from("product_type"), "SPOT".to_string()),
            ],
        };

        let mut wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-001"));
        let result = wrapper.get_string();

        assert!(result.contains("Connected"));
        assert!(result.contains("venue=BINANCE"));
        assert!(result.contains("product_type=SPOT"));
        assert!(result.ends_with('\n'));
        assert!(!result.contains("\x1b["));
    }

    #[rstest]
    fn test_log_line_wrapper_json_with_fields() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("RiskEngine"),
            message: "Order filled".to_string(),
            fields: smallvec::smallvec![
                (Ustr::from("strategy_id"), "S-001".to_string()),
                (Ustr::from("venue"), "BINANCE".to_string()),
            ],
        };

        let wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-001"));
        let json = wrapper.get_json();

        let parsed: Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["component"], "RiskEngine");
        assert_eq!(parsed["message"], "Order filled");
        assert_eq!(parsed["strategy_id"], "S-001");
        assert_eq!(parsed["venue"], "BINANCE");
    }

    #[rstest]
    fn test_log_line_wrapper_json_no_fields_has_no_extra_keys() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("Test"),
            message: "Simple".to_string(),
            fields: SmallVec::new(),
        };

        let wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-001"));
        let json = wrapper.get_json();

        let parsed: Value = serde_json::from_str(json.trim()).unwrap();
        let obj = parsed.as_object().unwrap();
        assert_eq!(obj.len(), 6); // timestamp, trader_id, level, color, component, message
    }

    #[rstest]
    fn test_log_line_wrapper_json_reserved_keys_not_overwritten() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Warn,
            color: LogColor::Normal,
            component: Ustr::from("Test"),
            message: "Real message".to_string(),
            fields: smallvec::smallvec![
                (Ustr::from("level"), "FAKE".to_string()),
                (Ustr::from("message"), "injected".to_string()),
                (Ustr::from("timestamp"), "bogus".to_string()),
                (Ustr::from("venue"), "BINANCE".to_string()),
            ],
        };

        let wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-001"));
        let json = wrapper.get_json();
        let parsed: Value = serde_json::from_str(json.trim()).unwrap();

        assert_eq!(parsed["level"], "WARN");
        assert_eq!(parsed["message"], "Real message");
        assert_ne!(parsed["timestamp"], "bogus");
        assert_eq!(parsed["venue"], "BINANCE");
    }

    #[rstest]
    fn test_log_line_wrapper_json_duplicate_extra_fields_last_value_wins() {
        let line = LogLine {
            timestamp: 1_650_000_000_000_000_000.into(),
            level: log::Level::Info,
            color: LogColor::Normal,
            component: Ustr::from("Test"),
            message: "Duplicate field".to_string(),
            fields: smallvec::smallvec![
                (Ustr::from("venue"), "BINANCE".to_string()),
                (Ustr::from("venue"), "OKX".to_string()),
            ],
        };

        let wrapper = LogLineWrapper::new(line, Ustr::from("TRADER-001"));
        let json = wrapper.get_json();
        let parsed: Value = serde_json::from_str(json.trim()).unwrap();

        assert_eq!(json.matches("\"venue\"").count(), 1);
        assert_eq!(parsed["venue"], "OKX");
    }

    /// Helper to convert module level map to sorted vec (descending by path length)
    fn sorted_module_filters(map: AHashMap<Ustr, LevelFilter>) -> Vec<(Ustr, LevelFilter)> {
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
        v
    }

    #[rstest]
    fn test_filter_no_filters_passes_all() {
        let module_filters = vec![];
        let component_level = AHashMap::new();

        assert!(!should_filter_log(
            &Ustr::from("anything"),
            Level::Trace,
            &module_filters,
            &component_level,
            false
        ));
    }

    #[rstest]
    fn test_filter_component_exact_match() {
        let module_filters = vec![];
        let component_level = AHashMap::from_iter([(Ustr::from("RiskEngine"), LevelFilter::Error)]);

        assert!(should_filter_log(
            &Ustr::from("RiskEngine"),
            Level::Info,
            &module_filters,
            &component_level,
            false
        ));
        assert!(!should_filter_log(
            &Ustr::from("RiskEngine"),
            Level::Error,
            &module_filters,
            &component_level,
            false
        ));
        assert!(!should_filter_log(
            &Ustr::from("Portfolio"),
            Level::Info,
            &module_filters,
            &component_level,
            false
        ));
    }

    #[rstest]
    fn test_filter_module_prefix_match() {
        let module_filters = vec![(Ustr::from("nautilus_okx::websocket"), LevelFilter::Debug)];
        let component_level = AHashMap::new();

        assert!(!should_filter_log(
            &Ustr::from("nautilus_okx::websocket"),
            Level::Debug,
            &module_filters,
            &component_level,
            false
        ));
        assert!(!should_filter_log(
            &Ustr::from("nautilus_okx::websocket::handler"),
            Level::Debug,
            &module_filters,
            &component_level,
            false
        ));
        assert!(should_filter_log(
            &Ustr::from("nautilus_okx::websocket::handler"),
            Level::Trace,
            &module_filters,
            &component_level,
            false
        ));
        assert!(!should_filter_log(
            &Ustr::from("nautilus_binance::data"),
            Level::Trace,
            &module_filters,
            &component_level,
            false
        ));
    }

    #[rstest]
    fn test_filter_longest_prefix_wins() {
        let module_filters = sorted_module_filters(AHashMap::from_iter([
            (Ustr::from("nautilus_okx"), LevelFilter::Error),
            (Ustr::from("nautilus_okx::websocket"), LevelFilter::Debug),
        ]));
        let component_level = AHashMap::new();

        assert!(!should_filter_log(
            &Ustr::from("nautilus_okx::websocket::handler"),
            Level::Debug,
            &module_filters,
            &component_level,
            false
        ));
        assert!(should_filter_log(
            &Ustr::from("nautilus_okx::data"),
            Level::Debug,
            &module_filters,
            &component_level,
            false
        ));
        assert!(!should_filter_log(
            &Ustr::from("nautilus_okx::data"),
            Level::Error,
            &module_filters,
            &component_level,
            false
        ));
    }

    #[rstest]
    fn test_filter_module_precedence_over_component() {
        let module_filters = vec![(Ustr::from("nautilus_okx::websocket"), LevelFilter::Debug)];
        let component_level =
            AHashMap::from_iter([(Ustr::from("nautilus_okx::websocket"), LevelFilter::Error)]);

        assert!(!should_filter_log(
            &Ustr::from("nautilus_okx::websocket"),
            Level::Debug,
            &module_filters,
            &component_level,
            false
        ));
    }

    #[rstest]
    fn test_filter_log_components_only_blocks_unknown() {
        let module_filters = vec![];
        let component_level = AHashMap::from_iter([(Ustr::from("RiskEngine"), LevelFilter::Debug)]);

        assert!(should_filter_log(
            &Ustr::from("Portfolio"),
            Level::Info,
            &module_filters,
            &component_level,
            true
        ));
        assert!(!should_filter_log(
            &Ustr::from("RiskEngine"),
            Level::Info,
            &module_filters,
            &component_level,
            true
        ));
    }

    #[rstest]
    fn test_filter_log_components_only_with_module() {
        let module_filters = vec![(Ustr::from("nautilus_okx"), LevelFilter::Debug)];
        let component_level = AHashMap::new();

        assert!(!should_filter_log(
            &Ustr::from("nautilus_okx::websocket"),
            Level::Debug,
            &module_filters,
            &component_level,
            true
        ));
        assert!(should_filter_log(
            &Ustr::from("nautilus_binance::data"),
            Level::Debug,
            &module_filters,
            &component_level,
            true
        ));
    }

    #[rstest]
    fn test_filter_level_comparison() {
        let module_filters = vec![];
        let component_level = AHashMap::from_iter([(Ustr::from("Test"), LevelFilter::Warn)]);

        assert!(!should_filter_log(
            &Ustr::from("Test"),
            Level::Error,
            &module_filters,
            &component_level,
            false
        ));
        assert!(!should_filter_log(
            &Ustr::from("Test"),
            Level::Warn,
            &module_filters,
            &component_level,
            false
        ));
        assert!(should_filter_log(
            &Ustr::from("Test"),
            Level::Info,
            &module_filters,
            &component_level,
            false
        ));
        assert!(should_filter_log(
            &Ustr::from("Test"),
            Level::Debug,
            &module_filters,
            &component_level,
            false
        ));
        assert!(should_filter_log(
            &Ustr::from("Test"),
            Level::Trace,
            &module_filters,
            &component_level,
            false
        ));
    }

    // These tests use global logging state (one logger per process).
    // They run correctly with cargo-nextest which isolates each test in its own process.
    //
    // Gated out under `cfg(madsim)`: every test here drives the file-logging writer
    // thread, which is itself gated out under simulation (see `Logger::init_with_config`),
    // so log events are dropped and these tests would either hang on `wait_until` or
    // assert against an empty log file. Logging is outside the determinism contract.
    #[cfg(not(all(feature = "simulation", madsim)))]
    mod serial_tests {
        use std::{sync::atomic::Ordering, time::Duration};

        use super::*;
        use crate::{
            logging::{
                LOGGING_BYPASSED, logging_clock_set_static_mode, logging_clock_set_static_time,
                logging_is_initialized, logging_set_bypass,
            },
            testing::wait_until,
        };

        #[rstest]
        fn test_logging_to_file() {
            let config = LoggerConfig {
                fileout_level: LevelFilter::Debug,
                ..Default::default()
            };

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                ..Default::default()
            };

            let log_guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config,
                file_config,
            );

            logging_clock_set_static_mode();
            logging_clock_set_static_time(1_650_000_000_000_000);

            log::info!(
                component = "RiskEngine";
                "This is a test"
            );

            let mut log_contents = String::new();

            wait_until(
                || {
                    std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .any(|entry| entry.path().is_file())
                },
                Duration::from_secs(3),
            );

            drop(log_guard); // Ensure log buffers are flushed

            wait_until(
                || {
                    let log_file_path = std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .find(|entry| entry.path().is_file())
                        .expect("No files found in directory")
                        .path();
                    log_contents = std::fs::read_to_string(log_file_path)
                        .expect("Error while reading log file");
                    !log_contents.is_empty()
                },
                Duration::from_secs(3),
            );

            assert_eq!(
                log_contents,
                "1970-01-20T02:20:00.000000000Z [INFO] TRADER-001.RiskEngine: This is a test\n"
            );
        }

        #[rstest]
        fn test_shutdown_drains_backlog_tail() {
            const N: usize = 1000;

            // Configure file logging at Info level
            let config = LoggerConfig {
                stdout_level: LevelFilter::Off,
                fileout_level: LevelFilter::Info,
                ..Default::default()
            };

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                ..Default::default()
            };

            let log_guard = Logger::init_with_config(
                TraderId::from("TRADER-TAIL"),
                UUID4::new(),
                config,
                file_config,
            )
            .expect("Failed to initialize logger");

            // Use static time for reproducibility
            logging_clock_set_static_mode();
            logging_clock_set_static_time(1_700_000_000_000_000);

            // Enqueue a known number of messages synchronously
            for i in 0..N {
                log::info!(component = "TailDrain"; "BacklogTest {i}");
            }

            // Drop guard to trigger shutdown (bypass + close + drain)
            drop(log_guard);

            // Wait until the file exists and contains at least N lines with our marker
            let mut count = 0usize;
            wait_until(
                || {
                    if let Some(log_file) = std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .find(|entry| entry.path().is_file())
                    {
                        let log_file_path = log_file.path();
                        if let Ok(contents) = std::fs::read_to_string(log_file_path) {
                            count = contents
                                .lines()
                                .filter(|l| l.contains("BacklogTest "))
                                .count();
                            count >= N
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                },
                Duration::from_secs(5),
            );

            assert_eq!(count, N, "Expected all pre-shutdown messages to be written");
        }

        #[rstest]
        fn test_log_component_level_filtering() {
            let config =
                LoggerConfig::from_spec("stdout=Info;fileout=Debug;RiskEngine=Error").unwrap();

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                ..Default::default()
            };

            let log_guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config,
                file_config,
            );

            logging_clock_set_static_mode();
            logging_clock_set_static_time(1_650_000_000_000_000);

            log::info!(
                component = "RiskEngine";
                "This is a test"
            );

            drop(log_guard); // Ensure log buffers are flushed

            wait_until(
                || {
                    if let Some(log_file) = std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .find(|entry| entry.path().is_file())
                    {
                        let log_file_path = log_file.path();
                        let log_contents = std::fs::read_to_string(log_file_path)
                            .expect("Error while reading log file");
                        !log_contents.contains("RiskEngine")
                    } else {
                        false
                    }
                },
                Duration::from_secs(3),
            );

            assert!(
                std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .any(|entry| entry.path().is_file()),
                "Log file exists"
            );
        }

        #[rstest]
        fn test_logging_to_file_in_json_format() {
            let config =
                LoggerConfig::from_spec("stdout=Info;is_colored;fileout=Debug;RiskEngine=Info")
                    .unwrap();

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                file_format: Some("json".to_string()),
                ..Default::default()
            };

            let log_guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config,
                file_config,
            );

            logging_clock_set_static_mode();
            logging_clock_set_static_time(1_650_000_000_000_000);

            log::info!(
                component = "RiskEngine";
                "This is a test"
            );

            let mut log_contents = String::new();

            drop(log_guard); // Ensure log buffers are flushed

            wait_until(
                || {
                    if let Some(log_file) = std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .find(|entry| entry.path().is_file())
                    {
                        let log_file_path = log_file.path();
                        log_contents = std::fs::read_to_string(log_file_path)
                            .expect("Error while reading log file");
                        !log_contents.is_empty()
                    } else {
                        false
                    }
                },
                Duration::from_secs(3),
            );

            assert_eq!(
                log_contents,
                "{\"timestamp\":\"1970-01-20T02:20:00.000000000Z\",\"trader_id\":\"TRADER-001\",\"level\":\"INFO\",\"color\":\"NORMAL\",\"component\":\"RiskEngine\",\"message\":\"This is a test\"}\n"
            );
        }

        #[rstest]
        fn test_init_sets_logging_is_initialized_flag() {
            let config = LoggerConfig::default();
            let file_config = FileWriterConfig::default();

            let guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config,
                file_config,
            );
            assert!(guard.is_ok());
            assert!(logging_is_initialized());

            drop(guard);
            assert!(!logging_is_initialized());
        }

        #[rstest]
        fn test_init_returns_error_when_log_guard_limit_reached() {
            let guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                LoggerConfig::default(),
                FileWriterConfig::default(),
            )
            .expect("Failed to initialize logger");

            LOGGING_GUARDS_ACTIVE.store(u8::MAX, Ordering::SeqCst);
            let result = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                LoggerConfig::default(),
                FileWriterConfig::default(),
            );
            LOGGING_GUARDS_ACTIVE.store(1, Ordering::SeqCst);
            drop(guard);

            assert_eq!(
                result.unwrap_err().to_string(),
                "Logging already initialized but new guard could not be created"
            );
        }

        #[rstest]
        fn test_reinit_after_guard_drop_fails() {
            let config = LoggerConfig::default();
            let file_config = FileWriterConfig::default();

            let guard1 = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config.clone(),
                file_config.clone(),
            );
            assert!(guard1.is_ok());
            drop(guard1);

            // Re-init fails because log crate's set_boxed_logger only works once per process
            let guard2 = Logger::init_with_config(
                TraderId::from("TRADER-002"),
                UUID4::new(),
                config,
                file_config,
            );
            assert!(guard2.is_err());
        }

        #[rstest]
        fn test_bypass_before_init_prevents_logging() {
            logging_set_bypass();
            assert!(LOGGING_BYPASSED.load(Ordering::Relaxed));

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let config = LoggerConfig {
                fileout_level: LevelFilter::Debug,
                ..Default::default()
            };
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                ..Default::default()
            };

            let guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config,
                file_config,
            );
            assert!(guard.is_ok());

            log::info!(
                component = "TestComponent";
                "This should be bypassed"
            );
            std::thread::sleep(Duration::from_millis(100));
            drop(guard);

            // Bypass flag remains permanently set (no reset mechanism)
            assert!(LOGGING_BYPASSED.load(Ordering::Relaxed));
        }

        #[rstest]
        fn test_module_level_filtering() {
            // Configure module-level filters (note: requires :: to be a module filter):
            // - nautilus::adapters=Warn (general adapter logs at Warn+)
            // - nautilus::adapters::okx=Debug (OKX adapter logs at Debug+)
            let config = LoggerConfig::from_spec(
                "stdout=Off;fileout=Trace;nautilus::adapters=Warn;nautilus::adapters::okx=Debug",
            )
            .unwrap();

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                ..Default::default()
            };

            let log_guard = Logger::init_with_config(
                TraderId::from("TRADER-MOD"),
                UUID4::new(),
                config,
                file_config,
            )
            .expect("Failed to initialize logger");

            logging_clock_set_static_mode();
            logging_clock_set_static_time(1_650_000_000_000_000);

            // Log from nautilus::adapters::okx::websocket - should pass (Debug allowed)
            log::debug!(
                component = "nautilus::adapters::okx::websocket";
                "OKX debug message"
            );

            // Log from nautilus::adapters::okx - should pass (Debug allowed)
            log::info!(
                component = "nautilus::adapters::okx";
                "OKX info message"
            );

            // Log from nautilus::adapters::binance - should be filtered (only Warn+ allowed)
            log::info!(
                component = "nautilus::adapters::binance";
                "Binance info message SHOULD NOT APPEAR"
            );

            // Log from nautilus::adapters::binance at Warn - should pass
            log::warn!(
                component = "nautilus::adapters::binance";
                "Binance warn message"
            );

            // Log from unrelated component - should pass (no filter)
            log::trace!(
                component = "Portfolio";
                "Portfolio trace message"
            );

            drop(log_guard);

            wait_until(
                || {
                    std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .any(|entry| entry.path().is_file())
                },
                Duration::from_secs(3),
            );

            let log_file_path = std::fs::read_dir(&temp_dir)
                .expect("Failed to read directory")
                .filter_map(Result::ok)
                .find(|entry| entry.path().is_file())
                .expect("No log file found")
                .path();

            let log_contents =
                std::fs::read_to_string(log_file_path).expect("Error reading log file");

            assert!(
                log_contents.contains("OKX debug message"),
                "OKX debug should pass (longer prefix wins)"
            );
            assert!(
                log_contents.contains("OKX info message"),
                "OKX info should pass"
            );
            assert!(
                log_contents.contains("Binance warn message"),
                "Binance warn should pass"
            );
            assert!(
                log_contents.contains("Portfolio trace message"),
                "Unfiltered component should pass"
            );
            assert!(
                !log_contents.contains("SHOULD NOT APPEAR"),
                "Binance info should be filtered (adapters=Warn)"
            );
        }

        #[rstest]
        fn test_logging_to_file_with_kv_fields() {
            let config = LoggerConfig {
                fileout_level: LevelFilter::Debug,
                ..Default::default()
            };

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                ..Default::default()
            };

            let log_guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config,
                file_config,
            );

            logging_clock_set_static_mode();
            logging_clock_set_static_time(1_650_000_000_000_000);

            log::info!(
                component = "DataEngine",
                venue = "BINANCE",
                product_type = "SPOT";
                "WebSocket connected"
            );

            let mut log_contents = String::new();

            drop(log_guard);

            wait_until(
                || {
                    if let Some(log_file) = std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .find(|entry| entry.path().is_file())
                    {
                        log_contents = std::fs::read_to_string(log_file.path())
                            .expect("Error while reading log file");
                        !log_contents.is_empty()
                    } else {
                        false
                    }
                },
                Duration::from_secs(3),
            );

            assert!(
                log_contents.contains("WebSocket connected"),
                "Message should be present"
            );
            assert!(
                log_contents.contains("venue=BINANCE"),
                "venue field should appear in output, was:\n{log_contents}"
            );
            assert!(
                log_contents.contains("product_type=SPOT"),
                "product_type field should appear in output, was:\n{log_contents}"
            );
        }

        #[rstest]
        fn test_logging_to_file_json_with_kv_fields() {
            let config =
                LoggerConfig::from_spec("stdout=Off;fileout=Debug;DataEngine=Debug").unwrap();

            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                file_format: Some("json".to_string()),
                ..Default::default()
            };

            let log_guard = Logger::init_with_config(
                TraderId::from("TRADER-001"),
                UUID4::new(),
                config,
                file_config,
            );

            logging_clock_set_static_mode();
            logging_clock_set_static_time(1_650_000_000_000_000);

            log::info!(
                component = "DataEngine",
                venue = "BINANCE",
                order_id = "O-12345";
                "Order filled"
            );

            let mut log_contents = String::new();

            drop(log_guard);

            wait_until(
                || {
                    if let Some(log_file) = std::fs::read_dir(&temp_dir)
                        .expect("Failed to read directory")
                        .filter_map(Result::ok)
                        .find(|entry| entry.path().is_file())
                    {
                        log_contents = std::fs::read_to_string(log_file.path())
                            .expect("Error while reading log file");
                        !log_contents.is_empty()
                    } else {
                        false
                    }
                },
                Duration::from_secs(3),
            );

            let parsed: serde_json::Value =
                serde_json::from_str(log_contents.trim()).expect("Should be valid JSON");
            assert_eq!(parsed["component"], "DataEngine");
            assert_eq!(parsed["message"], "Order filled");
            assert_eq!(parsed["venue"], "BINANCE");
            assert_eq!(parsed["order_id"], "O-12345");
        }
    }

    #[cfg(all(feature = "simulation", madsim))]
    mod sim_tests {
        use std::sync::atomic::Ordering;

        use super::*;
        use crate::logging::LOGGING_BYPASSED;

        #[rstest]
        fn test_init_under_madsim_skips_writer_thread_and_forces_bypass() {
            let config = LoggerConfig {
                bypass_logging: false,
                ..Default::default()
            };
            let temp_dir = tempdir().expect("Failed to create temporary directory");
            let file_config = FileWriterConfig {
                directory: Some(temp_dir.path().to_str().unwrap().to_string()),
                ..Default::default()
            };

            let _guard = Logger::init_with_config(
                TraderId::from("TRADER-SIM"),
                UUID4::new(),
                config,
                file_config,
            )
            .expect("init should succeed under simulation");

            assert!(LOGGING_INITIALIZED.load(Ordering::SeqCst));
            assert!(
                LOGGING_BYPASSED.load(Ordering::SeqCst),
                "bypass must be forced under cfg(madsim) even when config disables it"
            );
            assert!(
                LOGGER_HANDLE
                    .lock()
                    .expect("LOGGER_HANDLE mutex should not be poisoned")
                    .is_none(),
                "writer thread must not be spawned under cfg(madsim)"
            );
        }
    }
}
