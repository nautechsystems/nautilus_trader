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
    collections::VecDeque,
    fs::{File, create_dir_all},
    io::{self, BufWriter, Stderr, Stdout, Write},
    path::PathBuf,
    sync::OnceLock,
};

use chrono::Utc;
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

/// File rotation config.
#[derive(Debug, Clone)]
pub struct FileRotateConfig {
    /// Maximum file size in bytes before rotating.
    pub max_file_size: u64,
    /// Maximum number of backup files to keep.
    pub max_backup_count: u32,
    /// Current file size tracking.
    cur_file_size: u64,
    /// Queue of backup file paths (oldest first).
    backup_files: VecDeque<PathBuf>,
}

impl Default for FileRotateConfig {
    fn default() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024, // 10MB default
            max_backup_count: 5,
            cur_file_size: 0,
            backup_files: VecDeque::new(),
        }
    }
}

impl From<(u64, u32)> for FileRotateConfig {
    fn from(value: (u64, u32)) -> Self {
        let (max_file_size, max_backup_count) = value;
        Self {
            max_file_size,
            max_backup_count,
            cur_file_size: 0,
            backup_files: VecDeque::new(),
        }
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
    pub file_rotate: Option<FileRotateConfig>,
}

impl FileWriterConfig {
    /// Creates a new [`FileWriterConfig`] instance.
    #[must_use]
    pub fn new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
        file_rotate: Option<(u64, u32)>,
    ) -> Self {
        let file_rotate = file_rotate.map(FileRotateConfig::from);
        Self {
            directory,
            file_name,
            file_format,
            file_rotate,
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
        let utc_now = Utc::now();

        let basename = match file_config.file_name.as_ref() {
            Some(file_name) => {
                if file_config.file_rotate.is_some() {
                    let utc_datetime = utc_now.format("%Y-%m-%d_%H%M%S:%3f");
                    format!("{file_name}_{utc_datetime}")
                } else {
                    file_name.clone()
                }
            }
            None => {
                // Default base name
                let utc_component = if file_config.file_rotate.is_some() {
                    utc_now.format("%Y-%m-%d_%H%M%S:%3f")
                } else {
                    utc_now.format("%Y-%m-%d")
                };

                format!("{trader_id}_{utc_component}_{instance_id}")
            }
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
    pub fn should_rotate_file(&mut self) -> bool {
        self.file_config
            .file_rotate
            .as_ref()
            .map(|config| config.cur_file_size >= config.max_file_size)
            .unwrap_or(false)
    }

    fn rotate_file(&mut self) {
        // Flush current file
        self.flush();

        // Create new file
        let new_path = Self::create_log_file_path(
            &self.file_config,
            &self.trader_id,
            &self.instance_id,
            self.json_format,
        );
        match File::options().create(true).append(true).open(&new_path) {
            Ok(new_file) => {
                // Rotate existing file
                if let Some(rotate_config) = &mut self.file_config.file_rotate {
                    // Add current file to backup queue
                    rotate_config.backup_files.push_back(self.path.clone());
                    rotate_config.cur_file_size = 0;
                    cleanup_backups(rotate_config);
                }

                self.buf = BufWriter::new(new_file);
                self.path = new_path;
            }
            Err(e) => tracing::error!("Error creating log file: {e}"),
        }

        tracing::info!("Rotated log file, now logging to: {}", self.path.display());
    }
}

/// Clean up old backup files if we exceed the max backup count.
///
/// TODO: Minor consider using a more specific version to pop a single file
/// since normal execution will not create more than 1 excess file
fn cleanup_backups(rotate_config: &mut FileRotateConfig) {
    let files_to_remove = rotate_config
        .backup_files
        .len()
        .saturating_sub(rotate_config.max_backup_count as usize);

    // Remove oldest files if we exceed the max backup count
    rotate_config
        .backup_files
        .drain(..files_to_remove)
        .filter(|path| path.exists())
        .for_each(|path| match std::fs::remove_file(&path) {
            Ok(_) => tracing::debug!("Removed old log file: {}", path.display()),
            Err(e) => tracing::error!("Failed to remove old log file {}: {}", path.display(), e),
        });
}

impl LogWriter for FileWriter {
    fn write(&mut self, line: &str) {
        let line = strip_ansi_codes(line);
        let line_size = line.len() as u64;

        // Check if we need to rotate the file
        // Only rotate if the current file size plus the new line size exceeds max_file_size
        if let Some(rotate_config) = &mut self.file_config.file_rotate {
            if rotate_config.cur_file_size > 0
                && rotate_config.cur_file_size + line_size > rotate_config.max_file_size
            {
                self.rotate_file();
            }
        }

        match self.buf.write_all(line.as_bytes()) {
            Ok(()) => {
                // Update current file size
                if let Some(rotate_config) = &mut self.file_config.file_rotate {
                    rotate_config.cur_file_size += line_size;
                }
            }
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
