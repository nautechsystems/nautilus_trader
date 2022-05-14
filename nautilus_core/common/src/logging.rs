use std::{
    fmt::Display,
    io::{self, BufWriter, Stderr, Stdout, Write},
};

use pyo3::prelude::*;

#[pyclass]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum LogLevel {
    DBG,
    INF,
    WRN,
    ERR,
    CRT,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            LogLevel::DBG => "DEBUG",
            LogLevel::INF => "INFO",
            LogLevel::WRN => "WARNING",
            LogLevel::ERR => "ERROR",
            LogLevel::CRT => "CRITICAL",
        };
        write!(f, "{}", display)
    }
}

#[pyclass]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum LogFormat {
    HEADER,
    GREEN,
    BLUE,
    MAGENTA,
    CYAN,
    YELLOW,
    RED,
    ENDC,
    BOLD,
    UNDERLINE,
}

impl Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            LogFormat::HEADER => "\033[95m",
            LogFormat::GREEN => "\033[92m",
            LogFormat::BLUE => "\033[94m",
            LogFormat::MAGENTA => "\033[35m",
            LogFormat::CYAN => "\033[36m",
            LogFormat::YELLOW => "\033[1;33m",
            LogFormat::RED => "\033[1;31m",
            LogFormat::ENDC => "\033[0m",
            LogFormat::BOLD => "\033[1m",
            LogFormat::UNDERLINE => "\033[4m",
        };
        write!(f, "{}", display)
    }
}

#[pyclass]
pub struct Logger {
    trader_id: String,
    level_stdout: LogLevel,
    out: BufWriter<Stdout>,
    err: BufWriter<Stderr>,
}

#[pymethods]
impl Logger {
    #[new]
    fn new(trader_id: Option<String>, level_stdout: LogLevel) -> Self {
        Logger {
            trader_id: trader_id.unwrap_or("TRADER-000".to_string()),
            level_stdout,
            out: BufWriter::new(io::stdout()),
            err: BufWriter::new(io::stderr()),
        }
    }

    #[inline]
    fn log(
        &mut self,
        timestamp_ns: u64,
        level: LogLevel,
        color: LogFormat,
        component: String,
        msg: String,
    ) {
        let fmt_line = format!(
            "{bold}{dt}{startc} {color}[{level}] {trader_id}{component}: {msg}{endc}",
            bold = LogFormat::BOLD,
            dt = timestamp_ns,
            startc = LogFormat::ENDC,
            color = color,
            level = level,
            trader_id = self.trader_id,
            component = component,
            msg = msg,
            endc = LogFormat::ENDC,
        );
        if level >= LogLevel::ERR {
            self.out.write(fmt_line.as_bytes()).unwrap();
        } else if level >= self.level_stdout {
            self.err.write(fmt_line.as_bytes()).unwrap();
        }
    }

    fn debug(&mut self, timestamp_ns: u64, color: LogFormat, component: String, msg: String) {
        self.log(timestamp_ns, LogLevel::DBG, color, component, msg)
    }
    fn info(&mut self, timestamp_ns: u64, color: LogFormat, component: String, msg: String) {
        self.log(timestamp_ns, LogLevel::INF, color, component, msg)
    }
    fn warning(&mut self, timestamp_ns: u64, color: LogFormat, component: String, msg: String) {
        self.log(timestamp_ns, LogLevel::WRN, color, component, msg)
    }
    fn error(&mut self, timestamp_ns: u64, color: LogFormat, component: String, msg: String) {
        self.log(timestamp_ns, LogLevel::ERR, color, component, msg)
    }
    fn critical(&mut self, timestamp_ns: u64, color: LogFormat, component: String, msg: String) {
        self.log(timestamp_ns, LogLevel::CRT, color, component, msg)
    }
    fn flush(&mut self) {
        self.out.flush().unwrap();
        self.err.flush().unwrap();
    }
}
