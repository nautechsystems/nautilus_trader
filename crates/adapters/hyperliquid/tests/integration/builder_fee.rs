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

use std::process::Command;
#[cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

use rstest::rstest;

#[rstest]
#[case(None, "HYPERLIQUID_PK")]
#[case(Some("true"), "HYPERLIQUID_TESTNET_PK")]
#[case(Some("TRUE"), "HYPERLIQUID_TESTNET_PK")]
#[case(Some("True"), "HYPERLIQUID_TESTNET_PK")]
#[case(Some("1"), "HYPERLIQUID_TESTNET_PK")]
#[case(Some("false"), "HYPERLIQUID_PK")]
#[case(Some("FALSE"), "HYPERLIQUID_PK")]
#[case(Some("False"), "HYPERLIQUID_PK")]
#[case(Some("0"), "HYPERLIQUID_PK")]
fn test_network_selection(
    #[case] value: Option<&str>,
    #[case] key_var: &str,
    #[values(
        env!("CARGO_BIN_EXE_hyperliquid-builder-fee-approve"),
        env!("CARGO_BIN_EXE_hyperliquid-builder-fee-revoke")
    )]
    binary: &str,
) {
    let mut command = builder_command(binary);
    if let Some(value) = value {
        command.env("HYPERLIQUID_TESTNET", value);
    }

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("Error: {key_var} environment variable not set\n"),
    );
    assert_eq!(output.stderr, b"");
}

#[rstest]
#[case(env!("CARGO_BIN_EXE_hyperliquid-builder-fee-approve"))]
#[case(env!("CARGO_BIN_EXE_hyperliquid-builder-fee-revoke"))]
fn test_network_rejects_unknown_value(
    #[case] binary: &str,
    #[values("", "treu", "testnet", "mainnet", "prod", " true ", "2")] value: &str,
) {
    let output = builder_command(binary)
        .env("HYPERLIQUID_TESTNET", value)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stdout,
        b"Error: HYPERLIQUID_TESTNET must be true, false, 1, or 0\n",
    );
    assert_eq!(output.stderr, b"");
}

#[cfg(unix)]
#[rstest]
#[case(env!("CARGO_BIN_EXE_hyperliquid-builder-fee-approve"))]
#[case(env!("CARGO_BIN_EXE_hyperliquid-builder-fee-revoke"))]
fn test_network_rejects_non_unicode(#[case] binary: &str) {
    let value = OsString::from_vec(vec![0xff]);
    let output = builder_command(binary)
        .env("HYPERLIQUID_TESTNET", value)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stdout,
        b"Error: HYPERLIQUID_TESTNET must be true, false, 1, or 0\n",
    );
    assert_eq!(output.stderr, b"");
}

fn builder_command(binary: &str) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("--yes")
        .env_remove("HYPERLIQUID_TESTNET")
        .env_remove("HYPERLIQUID_PK")
        .env_remove("HYPERLIQUID_TESTNET_PK")
        .env("TOKIO_WORKER_THREADS", "1");
    command
}
