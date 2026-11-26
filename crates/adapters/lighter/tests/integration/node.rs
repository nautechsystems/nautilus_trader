// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use nautilus_common::enums::Environment;
use nautilus_lighter::{
    config::LighterExecutionClientConfig, factories::LighterExecutionClientFactory,
};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, SubmissionRecoveryPolicy},
};
use nautilus_model::identifiers::TraderId;
use rstest::rstest;

#[rstest]
#[case::default_with_lighter(None, true, Ok(()))]
#[case::retain_with_lighter(Some(SubmissionRecoveryPolicy::RetainUnresolved), true, Ok(()))]
#[case::resolve_with_lighter(
    Some(SubmissionRecoveryPolicy::ResolveLocally),
    true,
    Err(
        "Execution client 'LIGHTER-EXEC' requires submission_recovery_policy=RetainUnresolved, configured ResolveLocally"
    )
)]
#[case::resolve_without_lighter(Some(SubmissionRecoveryPolicy::ResolveLocally), false, Ok(()))]
fn test_live_node_validates_lighter_submission_recovery_policy(
    #[case] policy: Option<SubmissionRecoveryPolicy>,
    #[case] with_lighter: bool,
    #[case] expected: Result<(), &str>,
) {
    let mut builder =
        LiveNodeBuilder::new(TraderId::from("TRADER-001"), Environment::Live).unwrap();

    if let Some(policy) = policy {
        builder = builder.with_exec_engine_config(
            LiveExecutionEngineConfig::builder()
                .submission_recovery_policy(policy)
                .build(),
        );
    }

    if with_lighter {
        builder = builder
            .add_exec_client(
                Some("LIGHTER-EXEC".to_string()),
                Box::new(LighterExecutionClientFactory::new()),
                Box::new(LighterExecutionClientConfig::default()),
            )
            .unwrap();
    }

    assert_eq!(
        builder.build().map(drop).map_err(|e| e.to_string()),
        expected.map_err(str::to_owned)
    );
}
