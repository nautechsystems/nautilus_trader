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

//! Exit-code wiring for the `nautilus` binary.
//!
//! The `blockchain` subcommands only exist with the `defi` feature, so these tests compile only
//! under `--features defi`.

#![cfg(feature = "defi")]

use std::process::Command;

use rstest::rstest;

// `analyze-pools` exits non-zero when a pool fails, so batch jobs detect partial failures.
//
// Spawning the built binary is the only way to exercise `main`'s `run() -> ExitCode::FAILURE`
// mapping. With no RPC URL available, the single pool task fails at URL resolution,
// `run_analyze_pools` counts the failure and bails, and `main` maps that error to a non-zero
// exit. The child runs from a scratch directory with the RPC env vars removed so the failure is
// deterministic regardless of the host environment or a repository `.env`.
#[rstest]
fn analyze_pools_exits_non_zero_on_pool_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_nautilus"))
        .args([
            "blockchain",
            "analyze-pools",
            "--chain",
            "ethereum",
            "--dex",
            "UniswapV3",
            "--address",
            "0x1111111111111111111111111111111111111111",
            "--to-block",
            "100",
        ])
        .current_dir(std::env::temp_dir())
        .env_remove("INFURA_API_KEY")
        .env_remove("RPC_HTTP_URL")
        .output()
        .expect("failed to spawn nautilus binary");

    assert!(
        !output.status.success(),
        "analyze-pools should exit non-zero on pool failure, was {}",
        output.status
    );
}
