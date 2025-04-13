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

// Some constants not used (retained for completeness)
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use indexmap::IndexMap;

/// Common FIX tags used in this implementation.
pub mod fix_tag {
    // Standard header fields
    pub const BEGIN_STRING: u32 = 8; // FIX.4.4
    pub const BODY_LENGTH: u32 = 9; // Length of message body
    pub const MSG_TYPE: u32 = 35; // Message type
    pub const SENDER_COMP_ID: u32 = 49; // Sender's CompID
    pub const TARGET_COMP_ID: u32 = 56; // Target's CompID
    pub const MSG_SEQ_NUM: u32 = 34; // Message sequence number
    pub const SENDING_TIME: u32 = 52; // Message sending time
    pub const CHECKSUM: u32 = 10; // Checksum of message

    // Logon fields
    pub const ENCRYPT_METHOD: u32 = 98; // Encryption method (0 = none)
    pub const HEART_BT_INT: u32 = 108; // Heartbeat interval in seconds
    pub const RESET_SEQ_NUM_FLAG: u32 = 141; // Reset sequence numbers flag
    pub const USERNAME: u32 = 553; // Username for authentication
    pub const PASSWORD: u32 = 554; // Password for authentication

    // Execution report fields
    pub const CL_ORD_ID: u32 = 11; // Client order ID
    pub const ORIG_CL_ORD_ID: u32 = 41; // Original client order ID (for cancel/replace)
    pub const TRD_MATCH_ID: u32 = 880; // Trade match ID
    pub const EXEC_ID: u32 = 17; // Execution ID
    pub const EXEC_TRANS_TYPE: u32 = 20; // Execution transaction type
    pub const ORDER_ID: u32 = 37; // Order ID assigned by exchange
    pub const EXEC_TYPE: u32 = 150; // Execution type
    pub const ORD_STATUS: u32 = 39; // Order status
    pub const ORD_REJ_REASON: u32 = 103; // Order reject reason
    pub const SYMBOL: u32 = 55; // Symbol
    pub const SIDE: u32 = 54; // Order side
    pub const ORDER_QTY: u32 = 38; // Order quantity
    pub const ORD_TYPE: u32 = 40; // Order type
    pub const PRICE: u32 = 44; // Order price
    pub const STOP_PX: u32 = 99; // Stop price
    pub const STOP_LIMIT_PX: u32 = 3040; // Stop limit price
    pub const CURRENCY: u32 = 15; // Currency
    pub const TIME_IN_FORCE: u32 = 59; // Time in force
    pub const EXPIRE_TIME: u32 = 126; // Expiration time
    pub const EXEC_INST: u32 = 18; // Execution instructions
    pub const LAST_QTY: u32 = 32; // Last executed quantity
    pub const LAST_PX: u32 = 31; // Last executed price
    pub const LEAVES_QTY: u32 = 151; // Quantity open for further execution
    pub const CUM_QTY: u32 = 14; // Cumulative executed quantity
    pub const AVG_PX: u32 = 6; // Average execution price
    pub const TRANSACT_TIME: u32 = 60; // Transaction time
    pub const TEXT: u32 = 58; // Text message
    pub const LAST_LIQUIDITY_IND: u32 = 851; // Last liquidity indicator

    // Party identification fields
    pub const NO_PARTY_IDS: u32 = 453; // Number of party IDs
    pub const PARTY_ID: u32 = 448; // Party ID
    pub const PARTY_ID_SOURCE: u32 = 447; // Party ID source
    pub const PARTY_ROLE: u32 = 452; // Party role

    // Fee fields
    pub const NO_MISC_FEES: u32 = 136; // Number of miscellaneous fees
    pub const MISC_FEE_AMT: u32 = 137; // Miscellaneous fee amount
    pub const MISC_FEE_CURR: u32 = 138; // Miscellaneous fee currency
    pub const MISC_FEE_TYPE: u32 = 139; // Miscellaneous fee type

    // Coinbase specific fields
    pub const SELF_TRADE_PREVENTION_STRATEGY: u32 = 8000; // STP strategy
    pub const TARGET_STRATEGY: u32 = 847; // Target strategy (e.g., TWAP)
    pub const DEFAULT_APPL_VER_ID: u32 = 1137; // DefaultApplVerID
}

/// FIX message types.
pub(crate) mod fix_message_type {
    pub const HEARTBEAT: &str = "0";
    pub const TEST_REQUEST: &str = "1";
    pub const RESEND_REQUEST: &str = "2";
    pub const REJECT: &str = "3";
    pub const SEQUENCE_RESET: &str = "4";
    pub const LOGOUT: &str = "5";
    pub const EXECUTION_REPORT: &str = "8";
    pub const ORDER_CANCEL_REJECT: &str = "9";
    pub const LOGON: &str = "A";
    pub const NEWS: &str = "B";
    pub const EMAIL: &str = "C";
    pub const NEW_ORDER_SINGLE: &str = "D";
    pub const ORDER_CANCEL_REQUEST: &str = "F";
    pub const ORDER_CANCEL_REPLACE_REQUEST: &str = "G";
    pub const ORDER_STATUS_REQUEST: &str = "H";
    pub const BUSINESS_MESSAGE_REJECT: &str = "j";
}

/// Execution types for Execution Reports.
pub(crate) mod fix_exec_type {
    pub const NEW: &str = "0";
    pub const PARTIAL_FILL: &str = "1";
    pub const FILL: &str = "2";
    pub const CANCELED: &str = "4";
    pub const REPLACED: &str = "5";
    pub const PENDING_CANCEL: &str = "6";
    pub const REJECTED: &str = "8";
    pub const PENDING_NEW: &str = "A";
    pub const EXPIRED: &str = "C";
    pub const PENDING_REPLACE: &str = "E";
    pub const TRADE: &str = "F"; // For Trade Capture Reports
    pub const STOP_TRIGGERED: &str = "L";
}

pub(crate) const FIX_DELIMITER: u8 = b'\x01';

/// FIX message serialization/deserialization.
#[derive(Debug, Clone)]
pub(crate) struct FixMessage {
    fields: IndexMap<u32, String>,
}

impl FixMessage {
    /// Create a new FIX message with standard header.
    pub(crate) fn new(
        msg_type: &str,
        seq_num: usize,
        sender_comp_id: &str,
        target_comp_id: &str,
        now: &DateTime<Utc>,
    ) -> Self {
        let mut fields = IndexMap::new();

        // Standard header
        fields.insert(fix_tag::MSG_TYPE, msg_type.to_string());
        fields.insert(fix_tag::SENDER_COMP_ID, sender_comp_id.to_string());
        fields.insert(fix_tag::TARGET_COMP_ID, target_comp_id.to_string());
        fields.insert(fix_tag::MSG_SEQ_NUM, seq_num.to_string());

        // Add timestamp
        let timestamp = now.format("%Y%m%d-%H:%M:%S%.6f").to_string();
        fields.insert(fix_tag::SENDING_TIME, timestamp);

        Self { fields }
    }

    /// Gets the message type.
    pub(crate) fn msg_type(&self) -> Option<&str> {
        self.get_field(fix_tag::MSG_TYPE)
    }

    /// Gets the message sequence number
    pub(crate) fn msg_seq_num(&self) -> Option<usize> {
        self.get_field(fix_tag::MSG_SEQ_NUM)
            .and_then(|s| s.parse::<usize>().ok())
    }

    /// Gets a field from the message.
    pub fn get_field(&self, tag: u32) -> Option<&str> {
        self.fields.get(&tag).map(|s| s.as_str())
    }

    /// Adds a field to the message.
    pub fn add_field(&mut self, tag: u32, value: impl Into<String>) -> &mut Self {
        self.fields.insert(tag, value.into());
        self
    }

    /// Parses a FIX message from a byte slice.
    pub(crate) fn parse(data: &[u8]) -> Result<Self, String> {
        const DELIMITER: char = '\x01'; // Standard FIX delimiter (more efficent to define here)

        let data_str = std::str::from_utf8(data).map_err(|e| format!("Invalid UTF-8: {e}"))?;

        let mut fields = IndexMap::new();

        for field_str in data_str.split(DELIMITER) {
            if field_str.is_empty() {
                continue;
            }

            let parts: Vec<&str> = field_str.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(format!("Invalid field: {field_str}"));
            }

            let tag = parts[0]
                .parse::<u32>()
                .map_err(|e| format!("Invalid tag: {e}"))?;
            let value = parts[1].to_string();

            fields.insert(tag, value);
        }

        Ok(Self { fields })
    }

    /// Gets the value of a field by tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the tag is missing.
    pub(crate) fn get_field_checked(&self, tag: u32) -> anyhow::Result<&str> {
        self.get_field(tag)
            .ok_or(anyhow::anyhow!("Missing tag {tag}"))
    }

    /// Sets the value of a field by tag.
    fn set_field(&mut self, tag: u32, value: impl Into<String>) -> &mut Self {
        self.fields.insert(tag, value.into());
        self
    }

    /// Calculates body length and checksum, and build the final message bytes.
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        // Add BeginString
        let begin_string = self.get_field(fix_tag::BEGIN_STRING).unwrap_or("FIXT.1.1");
        buffer.extend_from_slice(format!("{}={}", fix_tag::BEGIN_STRING, begin_string).as_bytes());
        buffer.push(FIX_DELIMITER);

        let mut body_buffer = Vec::new();

        // Add all body fields
        for (&tag, value) in &self.fields {
            body_buffer.extend_from_slice(format!("{tag}={value}").as_bytes());
            body_buffer.push(FIX_DELIMITER);
        }

        // Calculate body length
        let body_length = body_buffer.len();
        buffer.extend_from_slice(format!("{}={}", fix_tag::BODY_LENGTH, body_length).as_bytes());
        buffer.push(FIX_DELIMITER);

        // Add body
        buffer.extend_from_slice(&body_buffer);

        // Calculate checksum
        let checksum: u32 = buffer.iter().map(|&b| b as u32).sum::<u32>() % 256;
        buffer.extend_from_slice(format!("{}={:03}", fix_tag::CHECKSUM, checksum).as_bytes());
        buffer.push(FIX_DELIMITER);

        buffer
    }

    /// Creates a logon message.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_logon(
        seq_num: usize,
        sender_comp_id: &str,
        target_comp_id: &str,
        heartbeat_interval: u64,
        username: &str,
        password: &str,
        text: &str,
        timestamp: &DateTime<Utc>,
    ) -> Self {
        let mut msg = Self::new(
            fix_message_type::LOGON,
            seq_num,
            sender_comp_id,
            target_comp_id,
            timestamp,
        );

        msg.add_field(fix_tag::ENCRYPT_METHOD, "0") // No encryption (must be 0)
            .add_field(fix_tag::HEART_BT_INT, heartbeat_interval.to_string())
            .add_field(fix_tag::RESET_SEQ_NUM_FLAG, "Y")
            .add_field(fix_tag::USERNAME, username)
            .add_field(fix_tag::PASSWORD, password)
            .add_field(fix_tag::TEXT, text)
            .add_field(fix_tag::DEFAULT_APPL_VER_ID, "9");

        msg
    }

    /// Creates a heartbeat message.
    pub(crate) fn create_heartbeat(
        seq_num: usize,
        sender_comp_id: &str,
        target_comp_id: &str,
        timestamp: &DateTime<Utc>,
    ) -> Self {
        Self::new(
            fix_message_type::HEARTBEAT,
            seq_num,
            sender_comp_id,
            target_comp_id,
            timestamp,
        )
    }

    /// Creates a logout message.
    pub(crate) fn create_logout(
        seq_num: usize,
        sender_comp_id: &str,
        target_comp_id: &str,
        text: Option<&str>,
        timestamp: &DateTime<Utc>,
    ) -> Self {
        let mut msg = Self::new(
            fix_message_type::LOGOUT,
            seq_num,
            sender_comp_id,
            target_comp_id,
            timestamp,
        );

        if let Some(text) = text {
            msg.add_field(fix_tag::TEXT, text);
        }

        msg
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_fix_message_to_bytes_simple() {
        let timestamp = Utc.with_ymd_and_hms(2025, 3, 22, 12, 34, 56).unwrap();
        let mut msg = FixMessage::new(fix_message_type::LOGON, 1, "SENDER", "TARGET", &timestamp);

        msg.add_field(fix_tag::ENCRYPT_METHOD, "0")
            .add_field(fix_tag::HEART_BT_INT, "10")
            .add_field(fix_tag::RESET_SEQ_NUM_FLAG, "Y");

        let bytes = msg.to_bytes();
        let message = String::from_utf8(bytes).unwrap();
        let expected = "8=FIXT.1.1\x019=76\x0135=A\x0149=SENDER\x0156=TARGET\x0134=1\x0152=20250322-12:34:56.000000\x0198=0\x01108=10\x01141=Y\x0110=137\x01";

        assert_eq!(message, expected);
    }

    #[rstest]
    fn test_fix_message_to_bytes_complete_logon() {
        let timestamp = Utc.with_ymd_and_hms(2025, 3, 22, 12, 34, 56).unwrap();
        let msg = FixMessage::create_logon(
            1,
            "SENDER",
            "TARGET",
            30,
            "username",
            "password",
            "signature",
            &timestamp,
        );

        let bytes = msg.to_bytes();
        let message = String::from_utf8(bytes).unwrap();
        let expected = "8=FIXT.1.1\x019=122\x0135=A\x0149=SENDER\x0156=TARGET\x0134=1\x0152=20250322-12:34:56.000000\x0198=0\x01108=30\x01141=Y\x01553=username\x01554=password\x0158=signature\x011137=9\x0110=253\x01";

        assert_eq!(message, expected);
    }
}
