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

//! Shared machinery for live order book stress harnesses.
//!
//! Book conformance means an adapter's emitted order book stays valid through venue and network
//! faults. [`BookStreamChecker`] defines that contract and ships with `nautilus-live` under the
//! `test-support` feature, so any test can apply it. The stress harnesses built on this module are
//! the development tool that exercises the contract live: run one when changing book sync or
//! recovery code. This is test source, not part of the published crate, and the venue harnesses do
//! not run in CI.
//!
//! In broad terms, a harness built on this module:
//!
//! - Connects a real data client to a real venue through a local fault-injecting proxy.
//! - Forces gaps, missing snapshots, disconnects, and subscription churn.
//! - Checks every emitted book against the contract and an independent venue oracle.
//! - Waits for each faulted book to recover and reports `PASS` or `FAIL`.
//!
//! The developer guide covers the validation levels, fault catalog, oracles, and harness
//! conventions: [Order book sync conformance](https://nautilustrader.io/docs/nightly/developer_guide/spec_data_testing/#order-book-sync-conformance).
//!
//! A venue harness at `crates/adapters/<venue>/tests/stress/book_stress.rs` includes this module
//! with `#[path = "../../../../live/tests/book/stress/mod.rs"]`, implements [`StressVenue`] with
//! its wire parsing, oracle, and instruments, writes its scenarios against [`Session`], and hands
//! them to [`run`]:
//!
//! - [`FaultProxy`] relays the adapter's WebSocket or line traffic to the venue and applies
//!   per-book [`Fault`] rules plus connection cuts and freezes.
//! - [`Session`] subscribes books, passes every emitted batch through [`BookStreamChecker`] and the
//!   venue oracle, waits for books to heal, and checks shutdown.
//! - [`run`] parses `--scenario`, `--timeout`, `--rounds`, and venue flags, runs the venue
//!   self-check, and reports one line per event: `START`, `ROUND`, `CHECK`, `SHUTDOWN`, and a final
//!   `PASS` or `FAIL`. A panic on any thread prints `FAIL` and exits with status 1.
//!
//! [`BookStreamChecker`]: nautilus_live::book::conformance::BookStreamChecker

#![allow(
    dead_code,
    unused_imports,
    reason = "each including harness uses a different part of this machinery"
)]

mod args;
mod oracle;
mod proxy;
mod session;

use std::{fmt::Display, sync::OnceLock};

use nautilus_common::{
    live::dst::{self, time::Instant},
    logging::{init_logging, logger::LoggerConfig, writer::FileWriterConfig},
};
use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;

pub(crate) use self::{
    args::{Flag, StressArgs},
    oracle::{WireBook, WireView, WireViews},
    proxy::{Fault, FaultProxy, FrameKind, LineStream, Route, Upstream, WireCodec, WireConnection},
    session::{BookProgress, Coverage, Session, StressVenue},
};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Runs a book stress harness and prints its final `PASS` line.
///
/// Parses the process arguments, printing usage and exiting with status 2 when they are invalid.
/// Installs a panic hook that prints one `FAIL` line and exits with status 1, runs
/// [`StressVenue::self_check`], and drives `harness` on a four-worker runtime. The fields
/// `harness` returns complete the `PASS` line.
///
/// # Panics
///
/// Panics if logging or the runtime cannot initialize.
#[allow(
    clippy::exit,
    reason = "the harness entry point reports invalid arguments through its exit status"
)]
pub(crate) fn run<V, F, Fut>(harness: F)
where
    V: StressVenue,
    F: FnOnce(StressArgs) -> Fut,
    Fut: Future<Output = String>,
{
    let raw = std::env::args().skip(1).collect::<Vec<_>>();
    let usage = args::usage(V::NAME, V::SCENARIOS, V::ROUNDS, V::FLAGS);

    if raw.iter().any(|arg| arg == "--help") {
        eprintln!("{usage}");
        std::process::exit(0);
    }

    let args = match StressArgs::parse(raw, V::SCENARIOS, V::ROUNDS, V::FLAGS) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}\n\n{usage}");
            std::process::exit(2);
        }
    };

    let _ = STARTED.set(Instant::now());

    let _log_guard = init_logging(
        TraderId::from("STRESS-001"),
        UUID4::new(),
        LoggerConfig {
            stdout_level: log::LevelFilter::Info,
            is_colored: false,
            ..LoggerConfig::default()
        },
        FileWriterConfig::default(),
    )
    .expect("logging initializes");

    install_fail_hook(V::NAME, args.scenario().to_string());
    emit(&[&format!("START venue={}", V::NAME), &args.to_string()]);
    V::self_check();
    check("self_check", "");

    let scenario = args.scenario().to_string();
    let runtime = dst::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime builds");
    let fields = runtime.block_on(harness(args));

    emit(&[
        &format!("PASS venue={} scenario={scenario}", V::NAME),
        &fields,
        &elapsed(),
    ]);
}

/// Prints a `CHECK` line for a probe that passed.
pub(crate) fn check(name: &str, fields: impl Display) {
    emit(&[
        &format!("CHECK name={name}"),
        &fields.to_string(),
        &elapsed(),
    ]);
}

fn emit(parts: &[&str]) {
    let line = parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("{line}");
}

fn elapsed() -> String {
    format!("elapsed_s={}", elapsed_secs())
}

fn elapsed_secs() -> u64 {
    STARTED
        .get()
        .map_or(0, |started| started.elapsed().as_secs())
}

#[allow(
    clippy::exit,
    reason = "a panic on any thread must end the run with a failing status"
)]
fn install_fail_hook(venue: &'static str, scenario: String) {
    let report = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        report(info);

        let payload = info.payload();
        let reason = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("panic");

        let location = info.location().map_or_else(String::new, |location| {
            format!(" at={}:{}", location.file(), location.line())
        });

        eprintln!(
            "FAIL venue={venue} scenario={scenario} reason={reason:?}{location} elapsed_s={}",
            elapsed_secs()
        );
        log::logger().flush();
        std::process::exit(1);
    }));
}
