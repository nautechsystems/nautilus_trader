// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

/// Determines if a JSON message is a subscription response from the blockchain RPC server.
///
/// Example response:
/// ```json
/// { "id": 1, "jsonrpc": "2.0", "result": "0x9cef478923ff08bf67fde6c64013158d"}
/// ```
#[must_use]
pub fn is_subscription_confirmation_response(json: &serde_json::Value) -> bool {
    json.get("id").is_some() && json.get("result").is_some()
}

/// Determines if a JSON message is a subscription event notification from the blockchain RPC server.
///
/// Example response:
/// ```json
/// {
///   "jsonrpc": "2.0", "method": "eth_subscription", "params": {
///     "subscription": "0x9cef478923ff08bf67fde6c64013158d",
///     "result": ...
///    }
/// }
/// ```
#[must_use]
pub fn is_subscription_event(json: &serde_json::Value) -> bool {
    json.get("method")
        .is_some_and(|value| value.as_str() == Some("eth_subscription"))
}

/// Extracts the subscription ID from a blockchain RPC subscription event notification.
#[must_use]
pub fn extract_rpc_subscription_id(json: &serde_json::Value) -> Option<&str> {
    json.get("params")
        .and_then(|params| params.get("subscription"))
        .and_then(|subscription| subscription.as_str())
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn subscription_confirmation() -> serde_json::Value {
        serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":1,"result":"0x4edabdfee3c542878dcc064c12151869"}"#,
        )
        .unwrap()
    }

    #[fixture]
    fn subscription_event() -> serde_json::Value {
        serde_json::from_str(r#"{"jsonrpc":"2.0","method":"eth_subscription",
        "params":{"subscription":"0x4edabdfee3c542878dcc064c12151869",
        "result":{"baseFeePerGas":"0x989680","difficulty":"0x1",
        "extraData":"0x5fcd3faec8b0c37571510e87ab402f0b7e6693ec607c880d38343e1884eb6823",
        "gasLimit":"0x4000000000000","gasUsed":"0x47a6d4",
        "hash":"0xb1e9f3e327e0686c9a299d9d6dbb6f2a77b60e1b948ddab9055bacbe02b7aee0",
        "miner":"0xa4b000000000000000000073657175656e636572",
        "mixHash":"0x00000000000231fe000000000154e0b000000000000000200000000000000000",
        "nonce":"0x00000000001dc3fe","number":"0x13a7cad4",
        "parentHash":"0x37356a864e9fd6eca0d4ebdd704739717f70e0e1f733b52317d377107c9b51ca",
        "receiptsRoot":"0x0604749e4d9c71de05e0a1661fa9b3eafeac1da1e98125dcc48015d9d9c5d0da",
        "sha3Uncles":"0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
        "stateRoot":"0x41e066985a516865c2d29a2fd5672f8de54b0459098a0d134ceb092e1e578e28",
        "timestamp":"0x680a58bf","totalDifficulty":"0x1254ed8c",
        "transactionsRoot":"0x1e5209d3a83f6315c74d5e39d59ad85420b51709b695473ee4f321147c356564"}}}"#
        ).unwrap()
    }

    #[rstest]
    fn test_is_subscription_confirmation_response(subscription_confirmation: serde_json::Value) {
        assert!(is_subscription_confirmation_response(
            &subscription_confirmation
        ));
    }

    #[rstest]
    fn test_is_subscription_event(subscription_event: serde_json::Value) {
        assert!(is_subscription_event(&subscription_event));
    }

    #[rstest]
    fn test_extract_subscription_id(subscription_event: serde_json::Value) {
        let id = extract_rpc_subscription_id(&subscription_event);
        assert_eq!(id, Some("0x4edabdfee3c542878dcc064c12151869"));
    }
}
