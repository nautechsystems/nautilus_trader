// -------------------------------------------------------------------------------------------------
// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// https://nautechsystems.io
// Licensed under the GNU Lesser General Public License Version 3.0.
// https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Measures offline fills through the real engine without allocation instrumentation.
use std::{hint::black_box, time::Instant};

use nautilus_execution::engine::ExecutionEngine;
use nautilus_model::events::OrderEventAny;

mod comparison;

fn main() -> std::process::ExitCode {
    comparison::run(process, "elapsed_ns")
}

fn process(engine: &mut ExecutionEngine, event: &OrderEventAny) -> u128 {
    let start = Instant::now();
    engine.process(black_box(event));
    start.elapsed().as_nanos()
}
