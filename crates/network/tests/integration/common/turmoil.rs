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

use std::time::Duration;

use turmoil::Builder;

#[allow(dead_code)]
const SOAK_START_ENV: &str = "NAUTILUS_TURMOIL_SOAK_START";
#[allow(dead_code)]
const SOAK_COUNT_ENV: &str = "NAUTILUS_TURMOIL_SOAK_COUNT";
#[allow(dead_code)]
const SOAK_PROGRESS_INTERVAL_ENV: &str = "NAUTILUS_TURMOIL_SOAK_PROGRESS_INTERVAL";
#[allow(dead_code)]
const DEFAULT_SOAK_PROGRESS_INTERVAL: u64 = 100;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct SeedSweep {
    next_seed: u64,
    iteration: u64,
    remaining: Option<u64>,
}

impl Iterator for SeedSweep {
    type Item = (u64, u64);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == Some(0) {
            return None;
        }

        let item = (self.iteration, self.next_seed);
        self.next_seed = self.next_seed.wrapping_add(1);
        self.iteration = self.iteration.wrapping_add(1);

        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
        }

        Some(item)
    }
}

#[must_use]
pub(crate) fn seeded_builder(seed: u64) -> Builder {
    let mut builder = Builder::new();
    builder.rng_seed(seed);
    builder
}

#[allow(dead_code)]
#[must_use]
pub(crate) fn seeded_builder_with_duration(seed: u64, duration: Duration) -> Builder {
    let mut builder = seeded_builder(seed);
    builder.simulation_duration(duration);
    builder
}

#[allow(dead_code)]
#[must_use]
pub(crate) fn stressed_builder(seed: u64, duration: Duration) -> Builder {
    let mut builder = seeded_builder_with_duration(seed, duration);
    builder.enable_random_order();
    builder.min_message_latency(Duration::from_millis(1));
    builder.max_message_latency(Duration::from_millis(25));
    builder
}

#[must_use]
#[allow(dead_code)]
pub(crate) fn seed_sweep_from_env() -> SeedSweep {
    SeedSweep {
        next_seed: env_u64(SOAK_START_ENV).unwrap_or(0),
        iteration: 0,
        remaining: env_u64(SOAK_COUNT_ENV),
    }
}

#[allow(dead_code)]
pub(crate) fn log_soak_seed(label: &str, iteration: u64, seed: u64) {
    let interval = env_u64(SOAK_PROGRESS_INTERVAL_ENV)
        .unwrap_or(DEFAULT_SOAK_PROGRESS_INTERVAL)
        .max(1);

    if iteration.is_multiple_of(interval) {
        eprintln!(
            "{label}: turmoil soak iteration {}, seed {seed:#018x}",
            iteration + 1
        );
    }
}

#[allow(dead_code)]
fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}
