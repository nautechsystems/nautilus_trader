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

use std::sync::Once;

use log::{Level, LevelFilter, Log, Metadata, Record};
use parking_lot::Mutex;

static CAPTURE: Capture = Capture {
    messages: Mutex::new(Vec::new()),
};
static INSTALL: Once = Once::new();
static CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Capture {
    messages: Mutex<Vec<(Level, String)>>,
}

impl Log for Capture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target().starts_with("nautilus_polymarket::data::")
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.messages
                .lock()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

pub(super) async fn capture_logs() -> tokio::sync::MutexGuard<'static, ()> {
    let guard = CAPTURE_LOCK.lock().await;
    INSTALL.call_once(|| log::set_logger(&CAPTURE).expect("test logger already installed"));
    log::set_max_level(LevelFilter::Debug);
    CAPTURE.messages.lock().clear();
    log::debug!(target: "nautilus_polymarket::data::test_logging", "Log capture control");
    assert_eq!(
        messages(),
        vec![(Level::Debug, "Log capture control".to_string())]
    );
    CAPTURE.messages.lock().clear();
    guard
}

pub(super) fn messages() -> Vec<(Level, String)> {
    CAPTURE.messages.lock().clone()
}
