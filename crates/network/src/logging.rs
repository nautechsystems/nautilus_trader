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

use std::fmt::Debug;

use nautilus_core::string::secret::{REDACTED, SecretString};
use url::Url;

/// A connection URL whose debug output contains only its HTTP or WebSocket origin.
///
/// Stores the original value without validation or normalization. Debug output omits user
/// information, paths, queries, and fragments; invalid URLs and other schemes are redacted.
/// Use only when the origin is public; use [`SecretString`] when the whole location is secret.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectionUrl(SecretString);

impl ConnectionUrl {
    /// Returns the original URL, which can contain credentials.
    ///
    /// Use [`Self::origin`] for logging.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// Returns the URL origin for logging, or [`REDACTED`] for invalid URLs and other schemes.
    #[must_use]
    pub fn origin(&self) -> String {
        url_origin(self.expose())
    }
}

impl From<String> for ConnectionUrl {
    fn from(value: String) -> Self {
        Self(value.into())
    }
}

impl From<&str> for ConnectionUrl {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl Debug for ConnectionUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.origin().fmt(f)
    }
}

/// Returns the scheme, host, and non-default port of an HTTP or WebSocket URL for logging.
///
/// Omits user information, paths, queries, and fragments, which can contain credentials.
/// Returns [`REDACTED`] for invalid URLs and other schemes. Hosts use their ASCII serialization.
#[must_use]
pub fn url_origin(value: &str) -> String {
    match Url::parse(value) {
        Ok(url) if matches!(url.scheme(), "http" | "https" | "ws" | "wss") => {
            url.origin().ascii_serialization()
        }
        _ => REDACTED.to_string(),
    }
}

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
mod url_origin_tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("wss://stream.example.com/feed", "wss://stream.example.com")]
    #[case("https://api.example.com:443/v1", "https://api.example.com")]
    #[case(
        "ws://user:password@example.com:8080/path-key?token=query-key#fragment-key",
        "ws://example.com:8080"
    )]
    #[case(
        "https://user%40secret:password%3Asecret@example.com/key%2Fsecret?api_key=secret",
        "https://example.com"
    )]
    #[case(
        "http://user:secret@[::1]:8123/path-secret?key=secret",
        "http://[::1]:8123"
    )]
    #[case("https://bücher.example/path-secret", "https://xn--bcher-kva.example")]
    #[case("wss://example.com:invalid/secret", REDACTED)]
    #[case("invalid-secret", REDACTED)]
    #[case("file:///secret", REDACTED)]
    #[case("blob:https://example.com/secret", REDACTED)]
    fn url_origin_preserves_endpoint_without_credentials(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let url = ConnectionUrl::from(input);

        assert_eq!(url_origin(input), expected);
        assert_eq!(url.origin(), expected);
        assert_eq!(format!("{url:?}"), format!("{expected:?}"));
        assert_eq!(ConnectionUrl::from(input.to_owned()), url);
        let cloned = url.clone();
        assert_eq!(url.expose(), input);
        drop(url);
        assert_eq!(cloned.expose(), input);
    }
}

#[cfg(test)]
#[cfg(not(all(feature = "simulation", madsim)))]
#[cfg(target_os = "linux")]
pub(crate) mod tests {
    use std::sync::Once;

    use log::{Level, LevelFilter, Log, Metadata, Record};
    use parking_lot::Mutex;

    struct CaptureState {
        targets: &'static [&'static str],
        messages: Vec<(Level, String)>,
    }

    struct CapturingLogger {
        state: Mutex<CaptureState>,
    }

    impl CapturingLogger {
        fn clear(&self, targets: &'static [&'static str]) {
            let mut state = self.state.lock();
            state.targets = targets;
            state.messages.clear();
        }

        fn messages(&self) -> Vec<(Level, String)> {
            self.state.lock().messages.clone()
        }
    }

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() <= Level::Trace
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                let mut state = self.state.lock();
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
