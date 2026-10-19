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

use std::{ffi::OsStr, process::Command};

use nautilus_common::live::runner::replace_data_event_sender;
use nautilus_model::identifiers::ClientId;
use rstest::rstest;

use crate::{
    common::{
        credential::Credential,
        enums::{LighterDeployment, LighterEnvironment},
    },
    config::LighterDataClientConfig,
    data::LighterDataClient,
};

const PRIVATE_KEY_HEX: &str =
    "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";
const PRIVATE_KEY_HEX_ALT: &str =
    "1c9f1074d35e9cbbdeae3abe5fab5c84d5b9e3cc9c27ed50bae8d3f2e4b9c201f9e4b5d6c7f80112";
const LIGHTER_ENV_CASE_VAR: &str = "NAUTILUS_LIGHTER_TEST_ENV_CASE";
const LIGHTER_ENV_RESTORE_VAR: &str = "NAUTILUS_LIGHTER_TEST_ENV_RESTORE";
const LIGHTER_ENV_VARS: [&str; 12] = [
    "LIGHTER_API_KEY_INDEX",
    "LIGHTER_API_SECRET",
    "LIGHTER_ACCOUNT_INDEX",
    "LIGHTER_TESTNET_API_KEY_INDEX",
    "LIGHTER_TESTNET_API_SECRET",
    "LIGHTER_TESTNET_ACCOUNT_INDEX",
    "LIGHTER_ROBINHOOD_API_KEY_INDEX",
    "LIGHTER_ROBINHOOD_API_SECRET",
    "LIGHTER_ROBINHOOD_ACCOUNT_INDEX",
    "LIGHTER_ROBINHOOD_TESTNET_API_KEY_INDEX",
    "LIGHTER_ROBINHOOD_TESTNET_API_SECRET",
    "LIGHTER_ROBINHOOD_TESTNET_ACCOUNT_INDEX",
];
const ENV_ABSENT: [Option<&str>; 12] = [None; 12];
const ENV_MAINNET_SECRET: [Option<&str>; 12] = [
    None,
    Some(PRIVATE_KEY_HEX),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];
const ENV_MAINNET_BLANK_SECRET: [Option<&str>; 12] = [
    None,
    Some("   "),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];
const ENV_MAINNET_INVALID_SECRET: [Option<&str>; 12] = [
    None,
    Some("not-hex"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];
const ENV_TESTNET: [Option<&str>; 12] = [
    None,
    None,
    None,
    Some("6"),
    Some(PRIVATE_KEY_HEX),
    Some("23456"),
    None,
    None,
    None,
    None,
    None,
    None,
];
const ENV_ROBINHOOD_MAINNET: [Option<&str>; 12] = [
    None,
    None,
    None,
    None,
    None,
    None,
    Some("7"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("34567"),
    None,
    None,
    None,
];
const ENV_ROBINHOOD_TESTNET: [Option<&str>; 12] = [
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some("8"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("45678"),
];
const ENV_PRESENT: [Option<&str>; 12] = [
    Some("5"),
    Some(PRIVATE_KEY_HEX),
    Some("12345"),
    Some("6"),
    Some(PRIVATE_KEY_HEX),
    Some("23456"),
    Some("7"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("34567"),
    Some("8"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("45678"),
];
const ENV_RESTORE_BASELINE: [Option<&str>; 12] = [
    Some("5"),
    Some(PRIVATE_KEY_HEX),
    Some("12345"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];
const ENV_PRESENT_ALT: [Option<&str>; 12] = [
    Some("7"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("34567"),
    Some("8"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("45678"),
    Some("9"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("56789"),
    Some("10"),
    Some(PRIVATE_KEY_HEX_ALT),
    Some("67890"),
];

const CREDENTIAL_ENV_CHILD: &str = "tests::credential_environment_child";
const CREDENTIAL_ENV_RESTORE_CHILD: &str = "tests::credential_environment_restore_child";
const CREDENTIAL_ENV_RESTORE_LEAF: &str = "tests::credential_environment_restore_leaf";

#[rstest]
fn credential_environment_cases_are_isolated() {
    let cases = [
        ("config_absent", ENV_ABSENT),
        ("config_partial", ENV_ABSENT),
        ("config_blank", ENV_ABSENT),
        ("config_testnet", ENV_TESTNET),
        ("config_mismatch", ENV_TESTNET),
        ("config_lighter_deployment_mismatch", ENV_ROBINHOOD_MAINNET),
        ("config_robinhood_mainnet", ENV_ROBINHOOD_MAINNET),
        ("config_robinhood_testnet", ENV_ROBINHOOD_TESTNET),
        ("config_robinhood_mismatch", ENV_RESTORE_BASELINE),
        ("credential_mainnet", ENV_PRESENT),
        ("credential_testnet", ENV_PRESENT),
        ("credential_robinhood_mainnet", ENV_PRESENT),
        ("credential_robinhood_testnet", ENV_PRESENT),
        (
            "credential_lighter_deployment_mismatch",
            ENV_ROBINHOOD_MAINNET,
        ),
        (
            "credential_robinhood_deployment_mismatch",
            ENV_RESTORE_BASELINE,
        ),
        ("credential_blank_fallback", ENV_MAINNET_SECRET),
        ("credential_blank_absent", ENV_ABSENT),
        ("credential_blank_env", ENV_MAINNET_BLANK_SECRET),
        ("credential_config_precedence", ENV_MAINNET_INVALID_SECRET),
        ("data_partial", ENV_ABSENT),
        ("data_config", ENV_ABSENT),
        ("data_blank_fallback", ENV_MAINNET_SECRET),
        ("data_robinhood_mainnet", ENV_ROBINHOOD_MAINNET),
        ("data_robinhood_testnet", ENV_ROBINHOOD_TESTNET),
    ];

    for (case, environment) in cases {
        let mut command = test_command(CREDENTIAL_ENV_CHILD, environment);
        command.env(LIGHTER_ENV_CASE_VAR, case);
        assert_child_succeeds(&mut command, case);
    }
}

#[rstest]
fn credential_environment_is_restored_after_normal_completion() {
    assert_restore_child_succeeds("normal");
}

#[rstest]
fn credential_environment_is_restored_after_panic_unwind() {
    assert_restore_child_succeeds("panic");
}

#[rstest]
#[ignore = "runs only in an isolated child process"]
fn credential_environment_child() {
    match std::env::var(LIGHTER_ENV_CASE_VAR).as_deref() {
        Ok("config_absent") => assert!(!LighterDataClientConfig::default().has_credentials()),
        Ok("config_partial") => assert_partial_configs_lack_credentials(),
        Ok("config_blank") => assert_blank_configs_lack_credentials(),
        Ok("config_testnet") => {
            let config = LighterDataClientConfig {
                environment: LighterEnvironment::Testnet,
                ..Default::default()
            };
            assert!(config.has_credentials());
        }
        Ok("config_mismatch") => {
            assert!(!LighterDataClientConfig::default().has_credentials());
        }
        Ok("config_lighter_deployment_mismatch") => {
            assert!(!LighterDataClientConfig::default().has_credentials());
        }
        Ok("config_robinhood_mainnet") => {
            let config = LighterDataClientConfig {
                deployment: LighterDeployment::Robinhood,
                ..Default::default()
            };
            assert!(config.has_credentials());
        }
        Ok("config_robinhood_testnet") => {
            let config = LighterDataClientConfig {
                environment: LighterEnvironment::Testnet,
                deployment: LighterDeployment::Robinhood,
                ..Default::default()
            };
            assert!(config.has_credentials());
        }
        Ok("config_robinhood_mismatch") => {
            let config = LighterDataClientConfig {
                deployment: LighterDeployment::Robinhood,
                ..Default::default()
            };
            assert!(!config.has_credentials());
        }
        Ok("credential_mainnet") => {
            assert_resolved_credentials(
                LighterDeployment::Lighter,
                LighterEnvironment::Mainnet,
                5,
                12_345,
                PRIVATE_KEY_HEX,
            );
        }
        Ok("credential_testnet") => {
            assert_resolved_credentials(
                LighterDeployment::Lighter,
                LighterEnvironment::Testnet,
                6,
                23_456,
                PRIVATE_KEY_HEX,
            );
        }
        Ok("credential_robinhood_mainnet") => {
            assert_resolved_credentials(
                LighterDeployment::Robinhood,
                LighterEnvironment::Mainnet,
                7,
                34_567,
                PRIVATE_KEY_HEX_ALT,
            );
        }
        Ok("credential_robinhood_testnet") => {
            assert_resolved_credentials(
                LighterDeployment::Robinhood,
                LighterEnvironment::Testnet,
                8,
                45_678,
                PRIVATE_KEY_HEX_ALT,
            );
        }
        Ok("credential_lighter_deployment_mismatch") => {
            let credential = Credential::resolve_for_deployment(
                None,
                None,
                None,
                LighterDeployment::Lighter,
                LighterEnvironment::Mainnet,
            )
            .unwrap();
            assert!(credential.is_none());
        }
        Ok("credential_robinhood_deployment_mismatch") => {
            let credential = Credential::resolve_for_deployment(
                None,
                None,
                None,
                LighterDeployment::Robinhood,
                LighterEnvironment::Mainnet,
            )
            .unwrap();
            assert!(credential.is_none());
        }
        Ok("credential_blank_fallback") => assert_blank_private_key_falls_back(),
        Ok("credential_blank_absent") => {
            let credential = Credential::resolve(
                Some("   ".to_string()),
                None,
                None,
                LighterEnvironment::Mainnet,
            )
            .unwrap();
            assert!(credential.is_none());
        }
        Ok("credential_blank_env") => {
            let credential =
                Credential::resolve(None, None, None, LighterEnvironment::Mainnet).unwrap();
            assert!(credential.is_none());
        }
        Ok("credential_config_precedence") => {
            let credential = Credential::resolve(
                Some(PRIVATE_KEY_HEX.to_string()),
                Some(12_345),
                Some(5),
                LighterEnvironment::Mainnet,
            )
            .unwrap()
            .unwrap();
            assert_private_key(&credential, PRIVATE_KEY_HEX);
        }
        Ok("data_partial") => {
            let config = LighterDataClientConfig {
                api_key_index: Some(5),
                private_key: Some(PRIVATE_KEY_HEX.into()),
                account_index: None,
                ..Default::default()
            };
            assert!(!create_data_client(config).has_credentials());
        }
        Ok("data_config") => {
            let config = LighterDataClientConfig {
                api_key_index: Some(5),
                account_index: Some(12_345),
                private_key: Some(PRIVATE_KEY_HEX.into()),
                ..Default::default()
            };
            assert!(create_data_client(config).has_credentials());
        }
        Ok("data_blank_fallback") => {
            let config = LighterDataClientConfig {
                api_key_index: Some(5),
                account_index: Some(12_345),
                private_key: Some("   ".into()),
                ..Default::default()
            };
            assert!(create_data_client(config).has_credentials());
        }
        Ok("data_robinhood_mainnet") => {
            let config = LighterDataClientConfig {
                deployment: LighterDeployment::Robinhood,
                ..Default::default()
            };
            assert!(create_data_client(config).has_credentials());
        }
        Ok("data_robinhood_testnet") => {
            let config = LighterDataClientConfig {
                environment: LighterEnvironment::Testnet,
                deployment: LighterDeployment::Robinhood,
                ..Default::default()
            };
            assert!(create_data_client(config).has_credentials());
        }
        _ => panic!("unknown isolated credential environment case"),
    }
}

#[rstest]
#[ignore = "runs only in an isolated child process"]
fn credential_environment_restore_child() {
    assert_environment(&ENV_RESTORE_BASELINE);
    let mode = std::env::var(LIGHTER_ENV_RESTORE_VAR).expect("restore mode must be set");
    let mut command = test_command(CREDENTIAL_ENV_RESTORE_LEAF, ENV_PRESENT_ALT);
    command.env(LIGHTER_ENV_RESTORE_VAR, &mode);
    let output = command.output().expect("restore leaf must run");

    assert!(output.status.success(), "{mode} restore leaf failed");
    assert_environment(&ENV_RESTORE_BASELINE);
}

#[rstest]
#[ignore = "runs only in an isolated child process"]
fn credential_environment_restore_leaf() {
    assert_environment(&ENV_PRESENT_ALT);

    match std::env::var(LIGHTER_ENV_RESTORE_VAR).as_deref() {
        Ok("normal") => {}
        Ok("panic") => {
            let result = std::panic::catch_unwind(|| {
                panic!("intentional isolated credential test panic");
            });
            assert!(result.is_err(), "isolated credential test did not unwind");
            assert_environment(&ENV_PRESENT_ALT);
        }
        _ => panic!("unknown restore mode"),
    }
}

fn assert_partial_configs_lack_credentials() {
    let configs = [
        (Some(5), None, None),
        (None, Some(12_345), None),
        (None, None, Some(PRIVATE_KEY_HEX.into())),
        (None, Some(12_345), Some(PRIVATE_KEY_HEX.into())),
        (Some(5), None, Some(PRIVATE_KEY_HEX.into())),
        (Some(5), Some(12_345), None),
    ];

    for (api_key_index, account_index, private_key) in configs {
        let config = LighterDataClientConfig {
            account_index,
            api_key_index,
            private_key,
            ..Default::default()
        };
        assert!(!config.has_credentials());
    }
}

fn assert_blank_configs_lack_credentials() {
    for private_key in ["", "   "] {
        let config = LighterDataClientConfig {
            api_key_index: Some(5),
            account_index: Some(12_345),
            private_key: Some(private_key.into()),
            ..Default::default()
        };
        assert!(!config.has_credentials());
    }
}

fn assert_resolved_credentials(
    deployment: LighterDeployment,
    environment: LighterEnvironment,
    api_key_index: u8,
    account_index: i64,
    private_key: &str,
) {
    let credential = Credential::resolve_for_deployment(None, None, None, deployment, environment)
        .unwrap()
        .unwrap();
    assert!(
        credential.api_key_index() == api_key_index,
        "resolved API key index differed",
    );
    assert!(
        credential.account_index() == account_index,
        "resolved account index differed",
    );
    assert_private_key(&credential, private_key);
}

fn assert_blank_private_key_falls_back() {
    for blank in ["", "   "] {
        let credential = Credential::resolve(
            Some(blank.to_string()),
            Some(12_345),
            Some(5),
            LighterEnvironment::Mainnet,
        )
        .unwrap()
        .unwrap();
        assert_private_key(&credential, PRIVATE_KEY_HEX);
    }
}

fn assert_private_key(credential: &Credential, expected: &str) {
    let expected = Credential::new(credential.api_key_index(), expected, 0).unwrap();
    assert!(
        credential.private_key().unwrap().to_le_bytes()
            == expected.private_key().unwrap().to_le_bytes(),
        "resolved private key differed",
    );
}

fn create_data_client(config: LighterDataClientConfig) -> LighterDataClient {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(sender);
    LighterDataClient::new(ClientId::new("LIGHTER"), config).unwrap()
}

fn assert_restore_child_succeeds(mode: &str) {
    let mut command = test_command(CREDENTIAL_ENV_RESTORE_CHILD, ENV_RESTORE_BASELINE);
    command.env(LIGHTER_ENV_RESTORE_VAR, mode);
    assert_child_succeeds(&mut command, mode);
}

fn test_command(test_name: &str, environment: [Option<&str>; 12]) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable must exist"));
    command.arg(test_name).arg("--exact").arg("--ignored");

    for (name, value) in LIGHTER_ENV_VARS.into_iter().zip(environment) {
        match value {
            Some(value) => {
                command.env(name, value);
            }
            None => {
                command.env_remove(name);
            }
        }
    }
    command
}

fn assert_child_succeeds(command: &mut Command, case: &str) {
    let output = command.output().expect("isolated test process must run");
    assert!(
        output.status.success(),
        "isolated credential environment case {case} failed",
    );
}

fn assert_environment(expected: &[Option<&str>; 12]) {
    let matches = LIGHTER_ENV_VARS
        .iter()
        .zip(expected)
        .all(|(name, value)| std::env::var_os(name).as_deref() == value.map(OsStr::new));
    assert!(matches, "credential environment was not restored");
}
