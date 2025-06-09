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

/// Extracts the transaction hash from a log entry
///
/// # Errors
///
/// Returns an error if the transaction hash is not present in the log.
pub fn extract_transaction_hash(
    log: &hypersync_client::simple_types::Log,
) -> anyhow::Result<String> {
    log.transaction_hash
        .as_ref()
        .map(|hash| hash.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing transaction hash in the log"))
}

/// Extracts the transaction index from a log entry
///
/// # Errors
///
/// Returns an error if the transaction index is not present in the log.
pub fn extract_transaction_index(log: &hypersync_client::simple_types::Log) -> anyhow::Result<u32> {
    log.transaction_index
        .as_ref()
        .map(|index| **index as u32)
        .ok_or_else(|| anyhow::anyhow!("Missing transaction index in the log"))
}

/// Extracts the log index from a log entry
///
/// # Errors
///
/// Returns an error if the log index is not present in the log.
pub fn extract_log_index(log: &hypersync_client::simple_types::Log) -> anyhow::Result<u32> {
    log.log_index
        .as_ref()
        .map(|index| **index as u32)
        .ok_or_else(|| anyhow::anyhow!("Missing log index in the log"))
}

/// Extracts the block number from a log entry
///
/// # Errors
///
/// Returns an error if the block number is not present in the log.
pub fn extract_block_number(log: &hypersync_client::simple_types::Log) -> anyhow::Result<u64> {
    log.block_number
        .as_ref()
        .map(|number| **number)
        .ok_or_else(|| anyhow::anyhow!("Missing block number in the log"))
}

/// Validates that a log entry corresponds to the expected event by comparing its topic0 with the provided event signature hash.
pub fn validate_event_signature_hash(
    event_name: &str,
    event_signature_hash: &str,
    log: &hypersync_client::simple_types::Log,
) -> anyhow::Result<()> {
    if let Some(topic) = log.topics.first().and_then(|t| t.as_ref()) {
        if hex::encode(topic) != event_signature_hash {
            anyhow::bail!("Invalid event signature for event '{event_name}'");
        }
    } else {
        anyhow::bail!("Missing event signature in topic0 for event '{event_name}'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hypersync_client::simple_types::Log;
    use rstest::*;
    use serde_json::json;

    use super::*;

    #[fixture]
    fn swap_log_1() -> Log {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b7e",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": [
                "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                "0x0000000000000000000000003fc91a3afd70395cd496c647d5a6cc9d4b2b7fad",
                "0x0000000000000000000000003fc91a3afd70395cd496c647d5a6cc9d4b2b7fad",
                null
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize log")
    }

    #[fixture]
    fn swap_log_2() -> Log {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": [
                "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                "0x00000000000000000000000066a9893cc07d91d95644aedd05d03f95e1dba8af",
                "0x000000000000000000000000f90321d0ecad58ab2b0c8c79db8aaeeefa023578",
                null
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize log")
    }

    #[fixture]
    fn log_without_topics() -> Log {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        serde_json::from_value(log_json).expect("Failed to deserialize log")
    }

    #[fixture]
    fn log_with_none_topic0() -> Log {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": [null]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize log")
    }

    #[rstest]
    fn test_validate_event_signature_hash_success(swap_log_1: Log) {
        // The topic0 from swap_log_1 is the swap event signature
        let expected_hash = "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

        let result = validate_event_signature_hash("Swap", expected_hash, &swap_log_1);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_event_signature_hash_success_log2(swap_log_2: Log) {
        // The topic0 from swap_log_2 is also the swap event signature
        let expected_hash = "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

        let result = validate_event_signature_hash("Swap", expected_hash, &swap_log_2);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_event_signature_hash_mismatch(swap_log_1: Log) {
        // Using a different event signature (e.g., Transfer event)
        let wrong_hash = "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

        let result = validate_event_signature_hash("Transfer", wrong_hash, &swap_log_1);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid event signature for event 'Transfer'"
        );
    }

    #[rstest]
    fn test_validate_event_signature_hash_missing_topic0(log_without_topics: Log) {
        let expected_hash = "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

        let result = validate_event_signature_hash("Swap", expected_hash, &log_without_topics);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing event signature in topic0 for event 'Swap'"
        );
    }

    #[rstest]
    fn test_validate_event_signature_hash_none_topic0(log_with_none_topic0: Log) {
        let expected_hash = "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

        let result = validate_event_signature_hash("Swap", expected_hash, &log_with_none_topic0);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing event signature in topic0 for event 'Swap'"
        );
    }

    #[rstest]
    fn test_extract_transaction_hash_success() {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_transaction_hash(&log);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        );
    }

    #[rstest]
    fn test_extract_transaction_hash_missing() {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_transaction_hash(&log);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing transaction hash in log"
        );
    }

    #[rstest]
    fn test_extract_transaction_index_success() {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": "0x5",
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_transaction_index(&log);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5u32);
    }

    #[rstest]
    fn test_extract_transaction_index_missing() {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_transaction_index(&log);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing transaction index in the log"
        );
    }

    #[rstest]
    fn test_extract_log_index_success() {
        let log_json = json!({
            "removed": null,
            "log_index": "0xa",
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_log_index(&log);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10u32);
    }

    #[rstest]
    fn test_extract_log_index_missing() {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_log_index(&log);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing log index in the log"
        );
    }

    #[rstest]
    fn test_extract_block_number_success() {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581b82",
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_block_number(&log);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 22551426u64); // 0x1581b82 in decimal
    }

    #[rstest]
    fn test_extract_block_number_missing() {
        let log_json = json!({
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": null,
            "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "data": "0x",
            "topics": []
        });
        let log: Log = serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = extract_block_number(&log);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing block number in the log"
        );
    }
}
