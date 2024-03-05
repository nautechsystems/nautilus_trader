// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
};

use chrono::{DateTime, Utc};
use log::LevelFilter;

use crate::logging::logger::LogLine;

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
    buf: BufWriter<Stdout>,
    level: LevelFilter,
}

impl StdoutWriter {
    pub fn new(level: LevelFilter, is_colored: bool) -> Self {
        Self {
            buf: BufWriter::new(io::stdout()),
            level,
            is_colored,
        }
    }
}

impl LogWriter for StdoutWriter {
    fn write(&mut self, line: &str) {
        match self.buf.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to stdout: {e:?}"),
        }
    }

    fn flush(&mut self) {
        match self.buf.flush() {
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
    buf: BufWriter<Stderr>,
}

impl StderrWriter {
    pub fn new(is_colored: bool) -> Self {
        Self {
            buf: BufWriter::new(io::stderr()),
            is_colored,
        }
    }
}

impl LogWriter for StderrWriter {
    fn write(&mut self, line: &str) {
        match self.buf.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to stderr: {e:?}"),
        }
    }

    fn flush(&mut self) {
        match self.buf.flush() {
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
}

impl FileWriterConfig {
    pub fn new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
    ) -> Self {
        Self {
            directory,
            file_name,
            file_format,
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
    pub fn new(
        trader_id: String,
        instance_id: String,
        file_config: FileWriterConfig,
        fileout_level: LevelFilter,
    ) -> Option<Self> {
        // Setup log file
        let json_format = match file_config.file_format.as_ref().map(|s| s.to_lowercase()) {
            Some(ref format) if format == "json" => true,
            None => false,
            Some(ref unrecognized) => {
                eprintln!(
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
                eprintln!("Error creating log file: {}", e);
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

    pub fn should_rotate_file(&self) -> bool {
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

        current_date_utc != creation_date_utc
    }
}

impl LogWriter for FileWriter {
    fn write(&mut self, line: &str) {
        if self.should_rotate_file() {
            self.flush();

            let file_path = Self::create_log_file_path(
                &self.file_config,
                &self.trader_id,
                &self.instance_id,
                self.json_format,
            );

            match File::options()
                .create(true)
                .append(true)
                .open(file_path.clone())
            {
                Ok(file) => {
                    self.buf = BufWriter::new(file);
                    self.path = file_path;
                }
                Err(e) => eprintln!("Error creating log file: {}", e),
            }
        }

        match self.buf.write_all(line.as_bytes()) {
            Ok(()) => {}
            Err(e) => eprintln!("Error writing to file: {e:?}"),
        }
    }

    fn flush(&mut self) {
        match self.buf.flush() {
            Ok(()) => {}
            Err(e) => eprintln!("Error flushing file: {e:?}"),
        }
    }

    fn enabled(&self, line: &LogLine) -> bool {
        line.level <= self.level
    }
}
