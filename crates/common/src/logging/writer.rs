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
    collections::VecDeque,
    fs::{File, create_dir_all},
    io::{self, BufWriter, Stderr, Stdout, Write},
    path::PathBuf,
    sync::OnceLock,
};

use chrono::{NaiveDate, Utc};
use log::LevelFilter;
use nautilus_core::consts::NAUTILUS_PREFIX;
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
        // Prevent error logs also writing to stdout (they go to stderr)
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
    /// Current file creation date.
    cur_file_creation_date: NaiveDate,
    /// Queue of backup file paths (oldest first).
    backup_files: VecDeque<PathBuf>,
}

impl Default for FileRotateConfig {
    fn default() -> Self {
        Self {
            max_file_size: 100 * 1024 * 1024, // 100MB default
            max_backup_count: 5,
            cur_file_size: 0,
            cur_file_creation_date: Utc::now().date_naive(),
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
            cur_file_creation_date: Utc::now().date_naive(),
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
    cur_file_date: NaiveDate,
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
                eprintln!(
                    "{NAUTILUS_PREFIX} Unrecognized log file format: {unrecognized}. Using plain text format as default."
                );
                false
            }
        };

        let file_path =
            match Self::create_log_file_path(&file_config, &trader_id, &instance_id, json_format) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("{NAUTILUS_PREFIX} Error creating log directory: {e}");
                    return None;
                }
            };

        match File::options()
            .create(true)
            .append(true)
            .open(file_path.clone())
        {
            Ok(file) => {
                // Seed cur_file_size from existing file length if rotation is enabled
                let mut file_config = file_config;
                if let Some(ref mut rotate_config) = file_config.file_rotate
                    && let Ok(metadata) = file.metadata()
                {
                    rotate_config.cur_file_size = metadata.len();
                }

                Some(Self {
                    json_format,
                    buf: BufWriter::new(file),
                    path: file_path,
                    file_config,
                    trader_id,
                    instance_id,
                    level: fileout_level,
                    cur_file_date: Utc::now().date_naive(),
                })
            }
            Err(e) => {
                eprintln!("{NAUTILUS_PREFIX} Error creating log file: {e}");
                None
            }
        }
    }

    fn create_log_file_path(
        file_config: &FileWriterConfig,
        trader_id: &str,
        instance_id: &str,
        is_json_format: bool,
    ) -> Result<PathBuf, io::Error> {
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
            create_dir_all(&file_path)?;
        }

        file_path.push(basename);
        file_path.set_extension(suffix);
        Ok(file_path)
    }

    #[must_use]
    fn should_rotate_file(&self, next_line_size: u64) -> bool {
        // Size-based rotation takes priority when configured
        if let Some(ref rotate_config) = self.file_config.file_rotate {
            rotate_config.cur_file_size + next_line_size > rotate_config.max_file_size
        // Otherwise, for default-named logs, rotate on UTC date change
        } else if self.file_config.file_name.is_none() {
            let today = Utc::now().date_naive();
            self.cur_file_date != today
        // No rotation for custom-named logs without size-based rotation
        } else {
            false
        }
    }

    fn rotate_file(&mut self) {
        self.flush();

        let new_path = match Self::create_log_file_path(
            &self.file_config,
            &self.trader_id,
            &self.instance_id,
            self.json_format,
        ) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("{NAUTILUS_PREFIX} Error creating log directory for rotation: {e}");
                return;
            }
        };

        match File::options().create(true).append(true).open(&new_path) {
            Ok(new_file) => {
                // Rotate existing file
                if let Some(rotate_config) = &mut self.file_config.file_rotate {
                    // Add current file to backup queue
                    rotate_config.backup_files.push_back(self.path.clone());
                    rotate_config.cur_file_size = 0;
                    rotate_config.cur_file_creation_date = Utc::now().date_naive();
                    cleanup_backups(rotate_config);
                } else {
                    // Update creation date for date-based rotation
                    self.cur_file_date = Utc::now().date_naive();
                }

                self.buf = BufWriter::new(new_file);
                self.path = new_path.clone();
                eprintln!(
                    "{NAUTILUS_PREFIX} Rotated log file, now logging to: {}",
                    new_path.display()
                );
            }
            Err(e) => eprintln!("{NAUTILUS_PREFIX} Error creating log file: {e}"),
        }
    }
}

/// Clean up old backup files if we exceed the max backup count.
///
/// TODO: Minor consider using a more specific version to pop a single file
/// since normal execution will not create more than 1 excess file
fn cleanup_backups(rotate_config: &mut FileRotateConfig) {
    // Remove oldest backup files until we are at or below max_backup_count
    let excess = rotate_config
        .backup_files
        .len()
        .saturating_sub(rotate_config.max_backup_count as usize);
    for _ in 0..excess {
        if let Some(path) = rotate_config.backup_files.pop_front() {
            if path.exists()
                && let Err(e) = std::fs::remove_file(&path)
            {
                eprintln!(
                    "{NAUTILUS_PREFIX} Failed to remove old log file {}: {e}",
                    path.display()
                );
            }
        } else {
            break;
        }
    }
}

impl LogWriter for FileWriter {
    fn write(&mut self, line: &str) {
        let line = strip_ansi_codes(line);
        let line_size = line.len() as u64;

        // Rotate file if needed (size-based or date-based depending on configuration)
        if self.should_rotate_file(line_size) {
            self.rotate_file();
        }

        match self.buf.write_all(line.as_bytes()) {
            Ok(()) => {
                // Update current file size
                if let Some(rotate_config) = &mut self.file_config.file_rotate {
                    rotate_config.cur_file_size += line_size;
                }
            }
            Err(e) => eprintln!("{NAUTILUS_PREFIX} Error writing to file: {e:?}"),
        }
    }

    fn flush(&mut self) {
        match self.buf.flush() {
            Ok(()) => {}
            Err(e) => eprintln!("{NAUTILUS_PREFIX} Error flushing file: {e:?}"),
        }

        match self.buf.get_ref().sync_all() {
            Ok(()) => {}
            Err(e) => eprintln!("{NAUTILUS_PREFIX} Error syncing file: {e:?}"),
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
    // Strip ANSI codes first (while \x1B is still present), then remove other control chars
    let no_ansi = re.replace_all(s, "");
    strip_nonprinting_except_newline(&no_ansi)
}

#[cfg(test)]
mod tests {
    use log::LevelFilter;
    use rstest::rstest;
    use tempfile::tempdir;

    use super::*;

    #[rstest]
    fn test_file_writer_with_rotation_creates_new_timestamped_file() {
        let temp_dir = tempdir().unwrap();

        let config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            file_name: Some("test".to_string()),
            file_format: None,
            file_rotate: Some(FileRotateConfig::from((2000, 5))),
        };

        let writer = FileWriter::new(
            "TRADER-001".to_string(),
            "instance-123".to_string(),
            config,
            LevelFilter::Info,
        )
        .unwrap();

        assert_eq!(
            writer
                .file_config
                .file_rotate
                .as_ref()
                .unwrap()
                .cur_file_size,
            0
        );
        assert!(writer.path.to_str().unwrap().contains("test_"));
    }

    #[rstest]
    #[case("Hello, World!", "Hello, World!")]
    #[case("Line1\nLine2", "Line1\nLine2")]
    #[case("Tab\there", "Tabhere")]
    #[case("Null\0char", "Nullchar")]
    #[case("DEL\u{7F}char", "DELchar")]
    #[case("Bell\u{07}sound", "Bellsound")]
    #[case("Mix\t\0\u{7F}ed", "Mixed")]
    fn test_strip_nonprinting_except_newline(#[case] input: &str, #[case] expected: &str) {
        let result = strip_nonprinting_except_newline(input);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("Plain text", "Plain text")]
    #[case("\x1B[31mRed\x1B[0m", "Red")]
    #[case("\x1B[1;32mBold Green\x1B[0m", "Bold Green")]
    #[case("Before\x1B[0mAfter", "BeforeAfter")]
    #[case("\x1B]0;Title\x07Content", "Content")]
    #[case("Text\t\x1B[31mRed\x1B[0m", "TextRed")]
    fn test_strip_ansi_codes(#[case] input: &str, #[case] expected: &str) {
        let result = strip_ansi_codes(input);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_file_writer_unwritable_directory_returns_none() {
        let config = FileWriterConfig {
            directory: Some("/nonexistent/path/that/should/not/exist".to_string()),
            file_name: Some("test".to_string()),
            file_format: None,
            file_rotate: None,
        };

        let writer = FileWriter::new(
            "TRADER-001".to_string(),
            "instance-123".to_string(),
            config,
            LevelFilter::Info,
        );

        assert!(writer.is_none());
    }

    #[rstest]
    fn test_file_writer_directory_is_file_returns_none() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("not_a_directory");
        std::fs::write(&file_path, "I am a file").unwrap();

        let config = FileWriterConfig {
            directory: Some(file_path.to_str().unwrap().to_string()),
            file_name: Some("test".to_string()),
            file_format: None,
            file_rotate: None,
        };

        let writer = FileWriter::new(
            "TRADER-001".to_string(),
            "instance-123".to_string(),
            config,
            LevelFilter::Info,
        );

        assert!(writer.is_none());
    }

    #[rstest]
    fn test_file_writer_unrecognized_format_defaults_to_text() {
        let temp_dir = tempdir().unwrap();

        let config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            file_name: Some("test".to_string()),
            file_format: Some("invalid_format".to_string()),
            file_rotate: None,
        };

        let writer = FileWriter::new(
            "TRADER-001".to_string(),
            "instance-123".to_string(),
            config,
            LevelFilter::Info,
        )
        .unwrap();

        assert!(!writer.json_format);
        assert!(writer.path.extension().unwrap() == "log");
    }

    #[rstest]
    fn test_file_writer_json_format() {
        let temp_dir = tempdir().unwrap();

        let config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            file_name: Some("test".to_string()),
            file_format: Some("json".to_string()),
            file_rotate: None,
        };

        let writer = FileWriter::new(
            "TRADER-001".to_string(),
            "instance-123".to_string(),
            config,
            LevelFilter::Info,
        )
        .unwrap();

        assert!(writer.json_format);
        assert!(writer.path.extension().unwrap() == "json");
    }

    #[rstest]
    fn test_stdout_writer_filters_error_level() {
        let writer = StdoutWriter::new(LevelFilter::Info, true);

        // Error level should NOT be enabled for stdout (goes to stderr)
        let error_line = LogLine {
            timestamp: 0.into(),
            level: log::Level::Error,
            color: crate::enums::LogColor::Normal,
            component: ustr::Ustr::from("Test"),
            message: "error".to_string(),
        };
        assert!(!writer.enabled(&error_line));

        // Info level should be enabled
        let info_line = LogLine {
            timestamp: 0.into(),
            level: log::Level::Info,
            color: crate::enums::LogColor::Normal,
            component: ustr::Ustr::from("Test"),
            message: "info".to_string(),
        };
        assert!(writer.enabled(&info_line));

        // Debug should NOT be enabled when stdout level is Info
        let debug_line = LogLine {
            timestamp: 0.into(),
            level: log::Level::Debug,
            color: crate::enums::LogColor::Normal,
            component: ustr::Ustr::from("Test"),
            message: "debug".to_string(),
        };
        assert!(!writer.enabled(&debug_line));
    }

    #[rstest]
    fn test_stderr_writer_only_enables_error_level() {
        let writer = StderrWriter::new(true);

        let error_line = LogLine {
            timestamp: 0.into(),
            level: log::Level::Error,
            color: crate::enums::LogColor::Normal,
            component: ustr::Ustr::from("Test"),
            message: "error".to_string(),
        };
        assert!(writer.enabled(&error_line));

        let warn_line = LogLine {
            timestamp: 0.into(),
            level: log::Level::Warn,
            color: crate::enums::LogColor::Normal,
            component: ustr::Ustr::from("Test"),
            message: "warn".to_string(),
        };
        assert!(!writer.enabled(&warn_line));
    }
}
