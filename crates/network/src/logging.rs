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

/// Logs that a task has started using `log::debug!`.
pub(crate) fn log_task_started(task_name: &str) {
    log::debug!("Started task '{task_name}'");
}

/// Logs that a task has stopped using `log::debug!`.
pub(crate) fn log_task_stopped(task_name: &str) {
    log::debug!("Stopped task '{task_name}'");
}

/// Logs that a task was aborted using `log::debug!`.
pub(crate) fn log_task_aborted(task_name: &str) {
    log::debug!("Aborted task '{task_name}'");
}

#[cfg(test)]
#[cfg(target_os = "linux")]
pub(crate) mod tests {
    use std::sync::{Mutex, Once};

    use log::{Level, LevelFilter, Log, Metadata, Record};

    struct CaptureState {
        targets: &'static [&'static str],
        messages: Vec<(Level, String)>,
    }

    struct CapturingLogger {
        state: Mutex<CaptureState>,
    }

    impl CapturingLogger {
        fn clear(&self, targets: &'static [&'static str]) {
            let mut state = self.state.lock().unwrap();
            state.targets = targets;
            state.messages.clear();
        }

        fn messages(&self) -> Vec<(Level, String)> {
            self.state.lock().unwrap().messages.clone()
        }
    }

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() <= Level::Trace
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                let mut state = self.state.lock().unwrap();
                if state.targets.is_empty() || state.targets.contains(&record.target()) {
                    state
                        .messages
                        .push((record.level(), record.args().to_string()));
                }
            }
        }

        fn flush(&self) {}
    }

    static CAPTURING_LOGGER: CapturingLogger = CapturingLogger {
        state: Mutex::new(CaptureState {
            targets: &[],
            messages: Vec::new(),
        }),
    };
    static CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static INSTALL_LOGGER: Once = Once::new();

    pub(crate) struct LogCapture {
        logger: &'static CapturingLogger,
        _guard: tokio::sync::MutexGuard<'static, ()>,
    }

    impl LogCapture {
        pub(crate) fn messages(&self) -> Vec<(Level, String)> {
            self.logger.messages()
        }
    }

    pub(crate) async fn capture_logs() -> LogCapture {
        capture_logs_for(&[]).await
    }

    pub(crate) async fn capture_logs_for(targets: &'static [&'static str]) -> LogCapture {
        let guard = CAPTURE_LOCK.lock().await;
        INSTALL_LOGGER.call_once(|| {
            log::set_logger(&CAPTURING_LOGGER).expect("test logger already installed");
        });
        log::set_max_level(LevelFilter::Trace);
        CAPTURING_LOGGER.clear(targets);
        LogCapture {
            logger: &CAPTURING_LOGGER,
            _guard: guard,
        }
    }
}
