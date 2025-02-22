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

use std::{
    fs::{create_dir_all, File},
    io::{self, BufWriter, Stderr, Stdout, Write},
    path::PathBuf,
    sync::OnceLock,
};

use chrono::{DateTime, Utc};
use log::LevelFilter;
use regex::Regex;

use crate::logging::logger::LogLine;

static ANSI_RE: OnceLock<Regex> = OnceLock::new();

pub trait LogWriter {
    /// Writes a log line.
    fn write(&mut self, line: &str);
    /// Flushes buffered logs.
    fn flush(&mut self);
    /// Checks if a line needs to be written to the writer or not.
    fn enabled(&self, line: &LogLine) -> bool;
}

#[derive(Debug)]
pub struct StdoutWriter {
    pub is_colored: bool,
    io: Stdout,
    level: LevelFilter,
}

impl StdoutWriter {
    /// Creates a new [`StdoutWriter`] instance.
    #[must_use]
    pub fn new(level: LevelFilter, is_colored: bool) -> Self {
        Self {
            io: io::stdout(),
            level,
            is_colored,
        }
    }
}

impl LogWriter for StdoutWriter {
    fn write(&mut self, line: &str) {
        match self.io.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to stdout: {e:?}"),
        }
    }

    fn flush(&mut self) {
        match self.io.flush() {
            Ok(()) => {}
            Err(e) => eprintln!("Error flushing stdout: {e:?}"),
        }
    }

    fn enabled(&self, line: &LogLine) -> bool {
        // Prevent error logs also writing to stdout
        line.level > LevelFilter::Error && line.level <= self.level
    }
}

#[derive(Debug)]
pub struct StderrWriter {
    pub is_colored: bool,
    io: Stderr,
}

impl StderrWriter {
    /// Creates a new [`StderrWriter`] instance.
    #[must_use]
    pub fn new(is_colored: bool) -> Self {
        Self {
            io: io::stderr(),
            is_colored,
        }
    }
}

impl LogWriter for StderrWriter {
    fn write(&mut self, line: &str) {
        match self.io.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to stderr: {e:?}"),
        }
    }

    fn flush(&mut self) {
        match self.io.flush() {
            Ok(()) => {}
            Err(e) => eprintln!("Error flushing stderr: {e:?}"),
        }
    }

    fn enabled(&self, line: &LogLine) -> bool {
        line.level == LevelFilter::Error
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug, Clone, Default)]
pub struct FileWriterConfig {
    pub directory: Option<String>,
    pub file_name: Option<String>,
    pub file_format: Option<String>,
    /// Maximum file size in bytes before rotating
    pub max_file_size: Option<u64>,
    /// Maximum number of backup files to keep
    pub max_backup_count: Option<u32>,
}

impl FileWriterConfig {
    /// Creates a new [`FileWriterConfig`] instance.
    #[must_use]
    pub const fn new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
        max_file_size: Option<u64>,
        max_backup_count: Option<u32>,
    ) -> Self {
        Self {
            directory,
            file_name,
            file_format,
            max_file_size: max_file_size.or(Some(100 * 1024 * 1024)), // Default 100MB
            max_backup_count: max_backup_count.or(Some(10)), // Default 10 backups
        }
    }
}

#[derive(Debug)]
pub struct FileWriter {
    pub json_format: bool,
    buf: BufWriter<File>,
    path: PathBuf,
    file_config: FileWriterConfig,
    trader_id: String,
    instance_id: String,
    level: LevelFilter,
}

impl FileWriter {
    /// Creates a new [`FileWriter`] instance.
    pub fn new(
        trader_id: String,
        instance_id: String,
        file_config: FileWriterConfig,
        fileout_level: LevelFilter,
    ) -> Option<Self> {
        // Set up log file
        let json_format = match file_config.file_format.as_ref().map(|s| s.to_lowercase()) {
            Some(ref format) if format == "json" => true,
            None => false,
            Some(ref unrecognized) => {
                tracing::error!(
                    "Unrecognized log file format: {unrecognized}. Using plain text format as default."
                );
                false
            }
        };

        let file_path =
            Self::create_log_file_path(&file_config, &trader_id, &instance_id, json_format);

        match File::options()
            .create(true)
            .append(true)
            .open(file_path.clone())
        {
            Ok(file) => Some(Self {
                json_format,
                buf: BufWriter::new(file),
                path: file_path,
                file_config,
                trader_id,
                instance_id,
                level: fileout_level,
            }),
            Err(e) => {
                tracing::error!("Error creating log file: {e}");
                None
            }
        }
    }

    fn create_log_file_path(
        file_config: &FileWriterConfig,
        trader_id: &str,
        instance_id: &str,
        is_json_format: bool,
    ) -> PathBuf {
        let basename = if let Some(file_name) = file_config.file_name.as_ref() {
            file_name.clone()
        } else {
            // default base name
            let current_date_utc = Utc::now().format("%Y-%m-%d");
            format!("{trader_id}_{current_date_utc}_{instance_id}")
        };

        let suffix = if is_json_format { "json" } else { "log" };
        let mut file_path = PathBuf::new();

        if let Some(directory) = file_config.directory.as_ref() {
            file_path.push(directory);
            create_dir_all(&file_path).expect("Failed to create directories for log file");
        }

        file_path.push(basename);
        file_path.set_extension(suffix);
        file_path
    }

    #[must_use]
    pub fn should_rotate_file(&self) -> bool {
        // Check date rotation
        let current_date_utc = Utc::now().date_naive();
        let metadata = self
            .path
            .metadata()
            .expect("Failed to read log file metadata");
        let creation_time = metadata
            .created()
            .expect("Failed to get log file creation time");

        let creation_time_utc: DateTime<Utc> = creation_time.into();
        let creation_date_utc = creation_time_utc.date_naive();

        // Check size rotation
        let file_size = metadata.len();
        let max_size = self.file_config.max_file_size.unwrap_or(100 * 1024 * 1024);

        current_date_utc != creation_date_utc || file_size >= max_size
    }

    fn rotate_file(&mut self) {
        self.flush();

        // Get current file path
        let current_path = self.path.clone();
        
        // Generate new file path
        let new_path = Self::create_log_file_path(
            &self.file_config,
            &self.trader_id,
            &self.instance_id,
            self.json_format,
        );

        // Rename current file with timestamp
        if let Some(max_backups) = self.file_config.max_backup_count {
            let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
            let backup_path = current_path.with_file_name(format!(
                "{}.{}.{}",
                current_path.file_stem().unwrap().to_str().unwrap(),
                timestamp,
                current_path.extension().unwrap().to_str().unwrap()
            ));

            if let Err(e) = std::fs::rename(&current_path, &backup_path) {
                tracing::error!("Error rotating log file: {e}");
                return;
            }

            // Clean up old backups
            self.cleanup_backups(&current_path, max_backups);
        }

        // Create new log file
        match File::options()
            .create(true)
            .append(true)
            .open(new_path.clone())
        {
            Ok(file) => {
                self.buf = BufWriter::new(file);
                self.path = new_path;
            }
            Err(e) => tracing::error!("Error creating log file: {e}"),
        }
    }

    fn cleanup_backups(&self, base_path: &PathBuf, max_backups: u32) {
        let dir = base_path.parent().unwrap();
        let file_stem = base_path.file_stem().unwrap().to_str().unwrap();

        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut backups: Vec<_> = entries
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    if path.file_stem()?.to_str()?.starts_with(file_stem) {
                        Some((entry.metadata().ok()?.modified().ok()?, path))
                    } else {
                        None
                    }
                })
                .collect();

            // Sort by modification time (oldest first)
            backups.sort_by_key(|(modified, _)| *modified);

            // Remove oldest backups if we have too many
            while backups.len() > max_backups as usize {
                if let Some((_, path)) = backups.first() {
                    if let Err(e) = std::fs::remove_file(path) {
                        tracing::error!("Error removing old log backup: {e}");
                    }
                    backups.remove(0);
                }
            }
        }
    }
}

impl LogWriter for FileWriter {
    fn write(&mut self, line: &str) {
        if self.should_rotate_file() {
            self.rotate_file();
        }

        let line = strip_ansi_codes(line);

        match self.buf.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => tracing::error!("Error writing to file: {e:?}"),
        }
    }

    fn flush(&mut self) {
        match self.buf.flush() {
            Ok(()) => {}
            Err(e) => tracing::error!("Error flushing file: {e:?}"),
        }
    }

    fn enabled(&self, line: &LogLine) -> bool {
        line.level <= self.level
    }
}

fn strip_nonprinting_except_newline(s: &str) -> String {
    s.chars()
        .filter(|&c| c == '\n' || (!c.is_control() && c != '\u{7F}'))
        .collect()
}

fn strip_ansi_codes(s: &str) -> String {
    let re = ANSI_RE.get_or_init(|| Regex::new(r"\x1B\[[0-9;?=]*[A-Za-z]|\x1B\].*?\x07").unwrap());
    let no_controls = strip_nonprinting_except_newline(s);
    re.replace_all(&no_controls, "").to_string()
}
