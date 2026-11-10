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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, config::ConfigError};
use nautilus_core::string::secret::REDACTED;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId, Venue},
};
use nautilus_okx::{
    common::enums::{OKXContractType, OKXEnvironment, OKXInstrumentType, OKXRegion},
    config::{OKXDataClientConfig, OKXExecutionClientConfig},
    execution::OKXExecutionClient,
    http::client::{OKXHttpClient, OKXRawHttpClient},
};
use rstest::rstest;

#[rstest]
#[case::empty(vec![], false)]
#[case::distinct(vec![OKXInstrumentType::Spot, OKXInstrumentType::Swap], false)]
#[case::adjacent(vec![OKXInstrumentType::Swap, OKXInstrumentType::Swap], true)]
#[case::separated(vec![OKXInstrumentType::Swap, OKXInstrumentType::Spot, OKXInstrumentType::Swap], true)]
fn test_config_duplicate_types(#[case] values: Vec<OKXInstrumentType>, #[case] duplicate: bool) {
    let data = OKXDataClientConfig {
        instrument_types: values.clone(),
        ..Default::default()
    };

    let execution = OKXExecutionClientConfig {
        instrument_types: values.clone(),
        ..Default::default()
    };

    let expected = if duplicate {
        Err(ConfigError::duplicate("instrument_types", None))
    } else {
        Ok(())
    };

    let data_result = data.validate();
    let execution_result = execution.validate();

    assert_eq!(data_result, expected);
    assert_eq!(execution_result, expected);
    assert_eq!(data.instrument_types, values);
    assert_eq!(execution.instrument_types, values);
}

const DATA_API_KEY: &str = "okx-data-api-key-sentinel";
const DATA_API_SECRET: &str = "okx-data-api-secret-sentinel";
const DATA_API_PASSPHRASE: &str = "okx-data-api-passphrase-sentinel";
const EXEC_API_KEY: &str = "okx-exec-api-key-sentinel";
const EXEC_API_SECRET: &str = "okx-exec-api-secret-sentinel";
const EXEC_API_PASSPHRASE: &str = "okx-exec-api-passphrase-sentinel";

#[rstest]
fn test_data_config_debug_redacts_credentials() {
    let config = OKXDataClientConfig {
        api_key: Some(DATA_API_KEY.into()),
        api_secret: Some(DATA_API_SECRET.into()),
        api_passphrase: Some(DATA_API_PASSPHRASE.into()),
        environment: OKXEnvironment::Demo,
        http_timeout_secs: 71,
        ..Default::default()
    };

    let debug_output = format!("{config:?}");
    let redacted = format!("Some({REDACTED})");

    assert!(!debug_output.contains(DATA_API_KEY));
    assert!(!debug_output.contains(DATA_API_SECRET));
    assert!(!debug_output.contains(DATA_API_PASSPHRASE));
    assert!(debug_output.contains(&format!("api_key: {redacted}")));
    assert!(debug_output.contains(&format!("api_secret: {redacted}")));
    assert!(debug_output.contains(&format!("api_passphrase: {redacted}")));
    assert!(debug_output.contains("environment: Demo"));
    assert!(debug_output.contains("http_timeout_secs: 71"));
}

#[rstest]
fn test_exec_config_debug_redacts_credentials() {
    let config = OKXExecutionClientConfig {
        account_id: AccountId::from("OKX-042"),
        api_key: Some(EXEC_API_KEY.into()),
        api_secret: Some(EXEC_API_SECRET.into()),
        api_passphrase: Some(EXEC_API_PASSPHRASE.into()),
        max_retries: 13,
        ..Default::default()
    };

    let debug_output = format!("{config:?}");
    let redacted = format!("Some({REDACTED})");

    assert!(!debug_output.contains(EXEC_API_KEY));
    assert!(!debug_output.contains(EXEC_API_SECRET));
    assert!(!debug_output.contains(EXEC_API_PASSPHRASE));
    assert!(debug_output.contains(&format!("api_key: {redacted}")));
    assert!(debug_output.contains(&format!("api_secret: {redacted}")));
    assert!(debug_output.contains(&format!("api_passphrase: {redacted}")));
    assert!(debug_output.contains("OKX-042"));
    assert!(debug_output.contains("max_retries: 13"));
}

#[rstest]
fn test_config_debug_handles_unset_and_partial_credentials() {
    let data_debug = format!("{:?}", OKXDataClientConfig::default());
    let exec_debug = format!(
        "{:?}",
        OKXExecutionClientConfig {
            api_secret: Some(String::new().into()),
            ..Default::default()
        }
    );

    assert!(data_debug.contains("api_key: None"));
    assert!(data_debug.contains("api_secret: None"));
    assert!(data_debug.contains("api_passphrase: None"));
    assert!(exec_debug.contains("api_key: None"));
    assert!(exec_debug.contains(&format!("api_secret: Some({REDACTED})")));
    assert!(exec_debug.contains("api_passphrase: None"));
}

#[rstest]
fn test_data_config_toml_minimal() {
    let config: OKXDataClientConfig = toml::from_str(
        r#"
environment = "demo"
instrument_types = ["SPOT", "SWAP"]
http_timeout_secs = 90
"#,
    )
    .unwrap();

    assert_eq!(config.environment, OKXEnvironment::Demo);
    assert_eq!(
        config.instrument_types,
        vec![OKXInstrumentType::Spot, OKXInstrumentType::Swap]
    );
    assert_eq!(config.http_timeout_secs, 90);
    assert!(!config.load_spreads);
    assert_eq!(config.book_stale_check_interval_secs, 5);
    assert_eq!(config.book_stale_threshold_secs, 30);
    assert_eq!(config.book_snapshot_timeout_secs, 10);
}

#[rstest]
fn test_data_config_toml_load_spreads() {
    let config: OKXDataClientConfig = toml::from_str(
        "
load_spreads = true
",
    )
    .unwrap();

    assert!(config.load_spreads);
}

#[rstest]
fn test_data_config_toml_book_stale_settings() {
    let config: OKXDataClientConfig = toml::from_str(
        "
book_stale_check_interval_secs = 2
book_stale_threshold_secs = 7
book_snapshot_timeout_secs = 4
",
    )
    .unwrap();

    assert_eq!(config.book_stale_check_interval_secs, 2);
    assert_eq!(config.book_stale_threshold_secs, 7);
    assert_eq!(config.book_snapshot_timeout_secs, 4);
}

#[rstest]
fn test_exec_config_toml_empty_uses_defaults() {
    let config: OKXExecutionClientConfig = toml::from_str("").unwrap();
    let expected = OKXExecutionClientConfig::default();
    assert_eq!(config.account_id, expected.account_id);
    assert_eq!(config.environment, expected.environment);
    assert_eq!(config.instrument_types, expected.instrument_types);
    assert_eq!(config.http_timeout_secs, expected.http_timeout_secs);
    assert_eq!(config.load_spreads, expected.load_spreads);
    assert_eq!(config.use_mm_mass_cancel, expected.use_mm_mass_cancel);
    assert_eq!(config.max_reconciliation_history_pages, None);
    assert_eq!(config.transport_backend, expected.transport_backend);
}

#[rstest]
#[case(0)]
#[case(3)]
#[case(51)]
#[case(u32::MAX)]
fn test_exec_config_toml_history_pages(#[case] pages: u32) {
    let config: OKXExecutionClientConfig =
        toml::from_str(&format!("max_reconciliation_history_pages = {pages}\n")).unwrap();
    let serialized = toml::Value::try_from(&config).unwrap();
    let roundtrip: OKXExecutionClientConfig = serialized.try_into().unwrap();

    assert_eq!(config.max_reconciliation_history_pages, Some(pages));
    assert_eq!(roundtrip.max_reconciliation_history_pages, Some(pages));
    assert_eq!(
        OKXExecutionClientConfig::default().max_reconciliation_history_pages,
        None
    );
}

#[rstest]
fn test_exec_config_toml_rejects_removed_fills_channel_key() {
    // use_fills_channel was removed: strict decoding must reject stale configs
    let result: Result<OKXExecutionClientConfig, _> = toml::from_str("use_fills_channel = true\n");
    assert!(result.is_err());
}

#[rstest]
fn test_exec_config_toml_load_spreads() {
    let config: OKXExecutionClientConfig = toml::from_str(
        "
load_spreads = true
",
    )
    .unwrap();

    assert!(config.load_spreads);
}

#[rstest]
fn test_data_config_default_region_is_global() {
    let config = OKXDataClientConfig::default();

    assert_eq!(config.region, OKXRegion::Global);
    assert_eq!(config.http_base_url(), "https://www.okx.com");
    assert_eq!(config.ws_public_url(), "wss://ws.okx.com/ws/v5/public");
}

#[rstest]
fn test_data_config_eea_region_urls() {
    let config = OKXDataClientConfig::builder()
        .region(OKXRegion::Eea)
        .build();

    assert_eq!(config.http_base_url(), "https://eea.okx.com");
    assert_eq!(config.ws_public_url(), "wss://wseea.okx.com/ws/v5/public");
    assert_eq!(
        config.ws_business_url(),
        "wss://wseea.okx.com/ws/v5/business"
    );
}

#[rstest]
fn test_exec_config_eea_region_urls() {
    let config = OKXExecutionClientConfig::builder()
        .region(OKXRegion::Eea)
        .build();

    assert_eq!(config.http_base_url(), "https://eea.okx.com");
    assert_eq!(config.ws_private_url(), "wss://wseea.okx.com/ws/v5/private");
    assert_eq!(
        config.ws_business_url(),
        "wss://wseea.okx.com/ws/v5/business"
    );
}

#[rstest]
fn test_config_region_override_takes_precedence() {
    let config = OKXDataClientConfig::builder()
        .region(OKXRegion::Eea)
        .environment(OKXEnvironment::Demo)
        .base_url_http("https://custom.proxy".to_string())
        .base_url_ws_public("wss://public.proxy:8443/ws/v5/public".to_string())
        .base_url_ws_business("wss://data.proxy:9443/ws/v5/business".to_string())
        .build();

    assert_eq!(config.http_base_url(), "https://custom.proxy");
    assert_eq!(
        config.ws_public_url(),
        "wss://public.proxy:8443/ws/v5/public"
    );
    assert_eq!(
        config.ws_business_url(),
        "wss://data.proxy:9443/ws/v5/business"
    );
}

#[rstest]
fn test_exec_config_region_override_takes_precedence() {
    let config = OKXExecutionClientConfig::builder()
        .region(OKXRegion::Us)
        .environment(OKXEnvironment::Demo)
        .base_url_ws_private("wss://private.proxy:8443/ws/v5/private".to_string())
        .base_url_ws_business("wss://execution.proxy:9443/ws/v5/business".to_string())
        .build();

    assert_eq!(
        config.ws_private_url(),
        "wss://private.proxy:8443/ws/v5/private"
    );
    assert_eq!(
        config.ws_business_url(),
        "wss://execution.proxy:9443/ws/v5/business"
    );
}

#[rstest]
fn test_data_config_toml_region() {
    let config: OKXDataClientConfig = toml::from_str(
        r#"
region = "eea"
"#,
    )
    .unwrap();

    assert_eq!(config.region, OKXRegion::Eea);
}

#[rstest]
fn test_exec_config_auth_timeout_secs() {
    assert_eq!(OKXExecutionClientConfig::default().auth_timeout_secs, None);

    let exec = OKXExecutionClientConfig::builder()
        .auth_timeout_secs(4)
        .build();
    assert_eq!(exec.auth_timeout_secs, Some(4));

    let exec: OKXExecutionClientConfig = toml::from_str("auth_timeout_secs = 8\n").unwrap();
    assert_eq!(exec.auth_timeout_secs, Some(8));
}

#[rstest]
#[case("contract_types")]
#[case("instrument_families")]
fn test_optional_selection_duplicates(#[case] field: &str) {
    let contract_types = (field == "contract_types").then(|| {
        vec![
            OKXContractType::Linear,
            OKXContractType::Inverse,
            OKXContractType::Linear,
        ]
    });

    let instrument_families = (field == "instrument_families").then(|| {
        vec![
            "BTC-USDT".to_string(),
            "ETH-USDT".to_string(),
            "BTC-USDT".to_string(),
        ]
    });

    let data = OKXDataClientConfig {
        contract_types: contract_types.clone(),
        instrument_families: instrument_families.clone(),
        ..Default::default()
    };

    let execution = OKXExecutionClientConfig {
        contract_types,
        instrument_families,
        ..Default::default()
    };

    let expected = Err(ConfigError::duplicate(field, None));

    assert_eq!(data.validate(), expected);
    assert_eq!(execution.validate(), expected);
}

#[rstest]
fn test_execution_client_rejects_duplicate_types() {
    let core = ExecutionClientCore::new(
        TraderId::from("TESTER-001"),
        ClientId::from("OKX"),
        Venue::from("OKX"),
        OmsType::Netting,
        AccountId::from("OKX-001"),
        AccountType::Margin,
        None,
        Rc::new(RefCell::new(Cache::default())),
    );

    let config = OKXExecutionClientConfig {
        instrument_types: vec![OKXInstrumentType::Swap, OKXInstrumentType::Swap],
        ..Default::default()
    };

    let error = OKXExecutionClient::new(core, config)
        .expect_err("duplicate config must fail before credentials or client setup");

    assert_eq!(
        error.downcast_ref::<ConfigError>(),
        Some(&ConfigError::duplicate("instrument_types", None))
    );
}

#[rstest]
#[case(0, 10_000, "retry_delay_initial_ms")]
#[case(2_000, 1_000, "retry_delay_max_ms")]
#[case(1_000, u64::MAX, "retry_delay_max_ms")]
fn test_config_rejects_invalid_retry_delays(
    #[case] initial: u64,
    #[case] maximum: u64,
    #[case] field: &str,
) {
    let data = OKXDataClientConfig {
        retry_delay_initial_ms: initial,
        retry_delay_max_ms: maximum,
        ..Default::default()
    };

    let execution = OKXExecutionClientConfig {
        retry_delay_initial_ms: initial,
        retry_delay_max_ms: maximum,
        ..Default::default()
    };

    for result in [data.validate(), execution.validate()] {
        let ConfigError::Range { field: actual, .. } =
            result.expect_err("invalid retries must fail validation")
        else {
            panic!("Expected a range error")
        };

        assert_eq!(actual, field);
    }
}

#[rstest]
#[case(None)]
#[case(Some(vec![]))]
fn test_config_requires_option_families(#[case] instrument_families: Option<Vec<String>>) {
    let data = OKXDataClientConfig {
        instrument_types: vec![OKXInstrumentType::Option],
        instrument_families: instrument_families.clone(),
        ..Default::default()
    };

    let execution = OKXExecutionClientConfig {
        instrument_types: vec![OKXInstrumentType::Option],
        instrument_families,
        ..Default::default()
    };

    let expected = Err(ConfigError::dependency(
        "instrument_types",
        "instrument_families",
        "OPTION requires at least one family",
    ));
    assert_eq!(data.validate(), expected);
    assert_eq!(execution.validate(), expected);
}

#[rstest]
#[case(OKXInstrumentType::Swap)]
#[case(OKXInstrumentType::Futures)]
#[case(OKXInstrumentType::Option)]
fn test_config_rejects_mixed_event_family_namespace(#[case] derivative: OKXInstrumentType) {
    let data = OKXDataClientConfig {
        instrument_types: vec![derivative, OKXInstrumentType::Events],
        instrument_families: Some(vec!["BTC-USD".to_string()]),
        ..Default::default()
    };

    let execution = OKXExecutionClientConfig {
        instrument_types: data.instrument_types.clone(),
        instrument_families: data.instrument_families.clone(),
        ..Default::default()
    };

    let expected = Err(ConfigError::invalid_value(
        "instrument_families",
        "EVENTS series and derivative families require separate clients",
    ));
    assert_eq!(data.validate(), expected);
    assert_eq!(execution.validate(), expected);
}

#[rstest]
#[case::public(false)]
#[case::authenticated(true)]
fn test_http_constructor_preserves_config_error(#[case] authenticated: bool) {
    let result = if authenticated {
        OKXHttpClient::with_credentials(
            None,
            None,
            None,
            None,
            60,
            0,
            0,
            1_000,
            OKXEnvironment::Demo,
            None,
        )
    } else {
        OKXHttpClient::new(None, 60, 0, 0, 1_000, OKXEnvironment::Demo, None)
    };

    let error =
        result.expect_err("invalid retries must fail before credentials or transport setup");
    assert_eq!(
        error.downcast_ref::<ConfigError>(),
        Some(&ConfigError::range(
            "retry_delay_initial_ms",
            "must be positive"
        ))
    );
}

#[rstest]
#[case::public(false)]
#[case::authenticated(true)]
fn test_raw_http_constructor_preserves_config_error(#[case] authenticated: bool) {
    let result = if authenticated {
        OKXRawHttpClient::with_credentials(
            "key".into(),
            "secret".into(),
            "passphrase".into(),
            "http://127.0.0.1:1".into(),
            60,
            0,
            0,
            1_000,
            OKXEnvironment::Demo,
            None,
        )
    } else {
        OKXRawHttpClient::new(None, 60, 0, 0, 1_000, OKXEnvironment::Demo, None)
    };

    let error = result.expect_err("invalid retries must fail before transport setup");
    assert_eq!(
        error.downcast_ref::<ConfigError>(),
        Some(&ConfigError::range(
            "retry_delay_initial_ms",
            "must be positive"
        ))
    );
}

#[rstest]
fn test_execution_config_rejects_mmp_only_mass_cancel() {
    let config = OKXExecutionClientConfig {
        use_mm_mass_cancel: true,
        ..Default::default()
    };

    assert_eq!(
        config.validate(),
        Err(ConfigError::unsupported_value(
            "use_mm_mass_cancel",
            "MMP-only family cancellation does not cancel ordinary orders"
        ))
    );
}
