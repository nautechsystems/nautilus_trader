use std::{
    fs::File,
    io::{self, BufWriter, Stderr, Stdout},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use log::LevelFilter;

use crate::logging::{FileWriterConfig, LogLine, LoggerConfig};

pub trait LogWriter {
    /// Writes a log line.
    fn write(&mut self, line: &str);
    /// Flushes buffered logs.
    fn flush(&mut self);
    /// Checks if a line needs to be written to the writer or not.
    fn enabled(&mut self, line: &LogLine, config: &LoggerConfig) -> bool;
}

#[derive(Debug)]
pub struct StdoutWriter {
    buf: BufWriter<Stdout>,
}

impl StdoutWriter {
    pub fn new() -> Self {
        Self {
            buf: BufWriter::new(io::stdout()),
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

    fn enabled(&mut self, line: &LogLine, config: &LoggerConfig) -> bool {
        line.level != LevelFilter::Error && line.level <= config.stdout_level
    }
}

#[derive(Debug)]
pub struct StderrWriter {
    buf: BufWriter<Stderr>,
}

impl StderrWriter {
    pub fn new() -> Self {
        Self {
            buf: BufWriter::new(io::stderr()),
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

    fn enabled(&mut self, line: &LogLine, config: &LoggerConfig) -> bool {
        line.level == LevelFilter::Error
    }
}

#[derive(Debug)]
pub struct FileWriter {
    buf: Option<BufWriter<File>>,
    path: PathBuf,
    file_config: FileWriterConfig,
}

impl FileWriter {
    pub fn new(file: Option<File>, path: PathBuf, file_config: FileWriterConfig) -> Self {
        Self {
            buf: file.map(BufWriter::new),
            path,
            file_config,
        }
    }
}

impl LogWriter for FileWriter {
    fn write(&mut self, line: &str) {
        if let Some(file_buf) = self.buf.as_mut() {
            match file_buf.write_all(line.as_bytes()) {
                Ok(()) => {}
                Err(e) => eprintln!("Error writing to file: {e:?}"),
            }
        }
    }

    fn flush(&mut self) {
        if let Some(file_buf) = self.buf.as_mut() {
            match file_buf.flush() {
                Ok(()) => {}
                Err(e) => eprintln!("Error flushing file: {e:?}"),
            }
        }
    }

    fn enabled(&mut self, line: &LogLine, config: &LoggerConfig) -> bool {
        config.fileout_level != LevelFilter::Off && line.level() <= config.fileout_level
    }
}

impl FileWriter {
    pub fn should_rotate_file(file_path: &Path, fileout_level: LevelFilter) -> bool {
        if fileout_level == LevelFilter::Off {
            false
        }

        if file_path.exists() {
            let current_date_utc = Utc::now().date_naive();
            let metadata = file_path
                .metadata()
                .expect("Failed to read log file metadata");
            let creation_time = metadata
                .created()
                .expect("Failed to get log file creation time");

            let creation_time_utc: DateTime<Utc> = creation_time.into();
            let creation_date_utc = creation_time_utc.date_naive();

            current_date_utc != creation_date_utc
        } else {
            false
        }
    }
}
