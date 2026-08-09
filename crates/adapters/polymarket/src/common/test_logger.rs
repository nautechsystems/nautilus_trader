// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at http://www.gnu.org/licenses/lgpl-3.0.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Shared log capture for adapter unit tests.

use std::sync::{Mutex, Once};

#[derive(Debug)]
struct CaptureLogger {
    records: Mutex<Vec<(log::Level, String)>>,
}

impl log::Log for CaptureLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        self.records
            .lock()
            .expect("capture logger mutex poisoned")
            .push((record.level(), record.args().to_string()));
    }

    fn flush(&self) {}
}

static LOGGER: CaptureLogger = CaptureLogger {
    records: Mutex::new(Vec::new()),
};
static LOGGER_INIT: Once = Once::new();

pub(crate) fn capture_start() -> usize {
    LOGGER_INIT.call_once(|| {
        log::set_logger(&LOGGER).expect("test logger already installed");
        log::set_max_level(log::LevelFilter::Trace);
    });
    LOGGER
        .records
        .lock()
        .expect("capture logger mutex poisoned")
        .len()
}

pub(crate) fn records_since(start: usize) -> Vec<(log::Level, String)> {
    LOGGER
        .records
        .lock()
        .expect("capture logger mutex poisoned")[start..]
        .to_vec()
}
