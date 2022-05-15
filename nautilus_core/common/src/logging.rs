use std::{
    fmt::Display,
    io::{self, BufWriter, Stderr, Stdout, Write},
};

use pyo3::{prelude::*, types::PyString, Python};

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
            LogFormat::HEADER => "\x1b[95m",
            LogFormat::GREEN => "\x1b[92m",
            LogFormat::BLUE => "\x1b[94m",
            LogFormat::MAGENTA => "\x1b[35m",
            LogFormat::CYAN => "\x1b[36m",
            LogFormat::YELLOW => "\x1b[1;33m",
            LogFormat::RED => "\x1b[1;31m",
            LogFormat::ENDC => "\x1b[0m",
            LogFormat::BOLD => "\x1b[1m",
            LogFormat::UNDERLINE => "\x1b[4m",
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
            trader_id: trader_id.unwrap_or_else(|| "TRADER-000".to_string()),
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
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        let fmt_line = format!(
            "{bold}{dt}{startc} {color}[{level}] {trader_id}{component}: {msg}{endc}\n",
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
            self.out.write_all(fmt_line.as_bytes())
        } else if level >= self.level_stdout {
            self.err.write_all(fmt_line.as_bytes())
        } else {
            Ok(())
        }
    }

    fn debug(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::DBG, color, component, msg)
    }
    fn info(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::INF, color, component, msg)
    }
    fn warning(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::WRN, color, component, msg)
    }
    fn error(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::ERR, color, component, msg)
    }
    fn critical(
        &mut self,
        timestamp_ns: u64,
        color: LogFormat,
        component: &PyString,
        msg: &PyString,
    ) -> Result<(), io::Error> {
        self.log(timestamp_ns, LogLevel::CRT, color, component, msg)
    }
    fn flush(&mut self) -> Result<(), io::Error> {
        self.out.flush()?;
        self.err.flush()
    }
}

pub fn register_module(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    let logging = PyModule::new(py, "logging")?;
    logging.add_class::<LogFormat>()?;
    logging.add_class::<LogLevel>()?;
    logging.add_class::<Logger>()?;

    m.add_submodule(logging)?;
    Ok(())
}
