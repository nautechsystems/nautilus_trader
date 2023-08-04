// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod http;
pub mod socket;
pub mod websocket;

use std::{collections::HashMap, fs::File, sync::Mutex};

use http::{HttpClient, HttpResponse};
use pyo3::prelude::*;
use socket::SocketClient;
use tracing::{instrument::WithSubscriber, metadata::LevelFilter, subscriber, Level, Subscriber};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    filter::filter_fn, fmt, layer::Filter, prelude::*, registry::LookupSpan, Registry,
};
use websocket::WebSocketClient;

// fn main() {
//     let rolling_log = RollingFileAppender::new(Rotation::NEVER, "hey", "cool.og");
//     let (non_blocking, _) = tracing_appender::non_blocking(rolling_log);
//     let layer1 = fmt::Layer::default()
//         .with_writer(non_blocking.with_max_level(Level::INFO));
//     let (non_blocking, _) = tracing_appender::non_blocking(std::io::stdout());
//     let layer2 = fmt::Layer::default()
//         .with_writer(non_blocking.with_max_level(Level::ERROR));

//     let top_level_filter: String = "module_a=info,module_b=error".to_string();
//     // can't add env_filter/top level filter
//     Registry::default().with(layer1).with(layer2).init();
//     // can't add multiple writer layers
//     fmt().with_env_filter(top_level_filter).init();
// }

fn set_global_tracing_collector(
    stdout_level: Option<Level>,
    stderr_level: Option<Level>,
    file_level: Option<(String, String, Level)>,
    // Max level of log allowed for a given target.
    // Target is the module/component name where the logic originated.
    // Default behaviour for a target not present in the hashmap is
    // to allow it.
    // The format for the string is target1=info,target2=debug. For e.g.
    // network=error,kernel=info
    target_filter: String,
) {
    let mut guards = Vec::new();
    let stdout_sub_builder = stdout_level.map(|stdout_level| {
        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
        guards.push(guard);
        fmt::Layer::default().with_writer(non_blocking.with_max_level(stdout_level))
    });
    let stderr_sub_builder = stderr_level.map(|stderr_level| {
        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
        guards.push(guard);
        fmt::Layer::default().with_writer(non_blocking.with_max_level(stderr_level))
    });
    let file_sub_builder = file_level.map(|(dir_path, file_prefix, file_level)| {
        let rolling_log = RollingFileAppender::new(Rotation::NEVER, dir_path, file_prefix);
        let (non_blocking, guard) = tracing_appender::non_blocking(rolling_log);
        guards.push(guard);
        fmt::Layer::default().with_writer(non_blocking.with_max_level(file_level))
    });

    // let target_filter = filter_fn(|metadata| {
    //     target_filter
    //         .get(metadata.target())
    //         .map(|target_level| target_level >= metadata.level())
    //         .unwrap_or(true)
    // });

    let subscriber = Registry::default();
    match ((stdout_sub_builder, stderr_sub_builder, file_sub_builder)) {
        (None, None, Some(a)) | (None, Some(a), None) | (Some(a), None, None) => {
            subscriber.with(a).init();
        }
        (None, Some(a), Some(b)) | (Some(a), None, Some(b)) | (Some(a), Some(b), None) => {
            subscriber.with(a).with(b).init();
        }
        (Some(a), Some(b), Some(c)) => subscriber.with(a).with(b).with(c).init(),
        (None, None, None) => {}
    }
}

/// Loaded as nautilus_pyo3.network
#[pymodule]
pub fn network(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<HttpClient>()?;
    m.add_class::<HttpResponse>()?;
    m.add_class::<WebSocketClient>()?;
    m.add_class::<SocketClient>()?;
    // m.add_class::<LogGuard>()?;
    Ok(())
}
