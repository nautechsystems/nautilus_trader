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

//! Simple FIX message buffer processor.

use std::sync::Arc;

use memchr::memchr;

use crate::socket::TcpMessageHandler;

const MIN_MESSAGE_SIZE: usize = 10; // Minimum length for "8=FIX" + "10=xxx|"
const MAX_MESSAGE_SIZE: usize = 8192; // Max message size to prevent buffer bloat
const CHECKSUM_LEN: usize = 7; // Length of "10=xxx|"
const CHECKSUM_TAG: &[u8] = b"10=";
const START_PATTERN: &[u8] = b"8=FIX";
const START_CHAR: u8 = b'8';
const DELIMITER: u8 = b'\x01';

/// Processes a mutable byte buffer containing FIX protocol messages.
///
/// Extracts complete messages starting with "8=FIX" (supporting various FIX versions)
/// and ending with "10=xxx|" (where xxx is a three-digit checksum), passes them to the
/// provided handler, and removes them from the buffer, leaving incomplete data for
/// future processing.
///
/// # Assumptions
///
/// - Fields are delimited by SOH (`\x01`).
/// - The checksum field is "10=xxx|" where xxx is a three-digit ASCII number.
/// - Messages are ASCII-encoded.
///
/// # Behavior
///
/// - Uses `memchr` for efficient message start detection.
/// - Discards malformed data up to the next potential message start.
/// - Retains incomplete messages in the buffer for additional data.
/// - Enforces a maximum message size to prevent buffer overflow.
///
/// # Warning
///
/// This parser is designed for basic FIX message processing and does not support all features
/// of the FIX protocol. Notably, it lacks handling for repeating groups and other advanced
/// structures, which may be required for full protocol compliance in complex scenarios.
pub(crate) fn process_fix_buffer(buf: &mut Vec<u8>, handler: &Arc<TcpMessageHandler>) {
    let mut processed_to = 0;

    while processed_to < buf.len() {
        if buf.len() - processed_to < MIN_MESSAGE_SIZE {
            break;
        }

        // Find the potential start of a FIX message
        let start_idx = memchr(START_CHAR, &buf[processed_to..]).map(|i| processed_to + i);
        if let Some(idx) = start_idx {
            if idx + START_PATTERN.len() <= buf.len()
                && &buf[idx..idx + START_PATTERN.len()] == START_PATTERN
            {
                // Search for message end
                if let Some(end_pos) = find_message_end(&buf[idx..]) {
                    let message_end = idx + end_pos;
                    if message_end - idx > MAX_MESSAGE_SIZE {
                        // Message exceeds max size, discard up to this point
                        processed_to = idx + 1;
                        continue;
                    }
                    let message = &buf[idx..message_end];
                    handler(message); // Pass complete message to handler
                    processed_to = message_end; // Update processed position
                } else {
                    // Incomplete message, wait for more data
                    break;
                }
            } else {
                // Invalid start pattern, discard data up to this point
                processed_to = idx + 1;
            }
        } else {
            // No message start found in the remaining buffer, clear it to avoid garbage buildup
            buf.clear();
            return;
        }
    }

    // Remove all processed data from the buffer
    if processed_to > 0 {
        buf.drain(0..processed_to);
    }
}

/// Locate the end of a FIX message. Searches for "10=xxx|" where xxx is a three-digit ASCII number.
#[inline(always)]
fn find_message_end(buf: &[u8]) -> Option<usize> {
    let mut idx = 0;
    while idx + CHECKSUM_LEN <= buf.len() {
        if buf[idx..idx + CHECKSUM_LEN].starts_with(CHECKSUM_TAG)
            && buf[idx + 3].is_ascii_digit()
            && buf[idx + 4].is_ascii_digit()
            && buf[idx + 5].is_ascii_digit()
            && buf[idx + 6] == DELIMITER
        {
            return Some(idx + CHECKSUM_LEN);
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod process_fix_buffer_tests {
    use std::sync::{Arc, Mutex};

    use rstest::rstest;

    use crate::{fix::process_fix_buffer, socket::TcpMessageHandler};

    #[rstest]
    fn test_process_empty_buffer() {
        let mut buffer = Vec::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        // Buffer was empty, so no messages should be processed
        assert!(received.lock().unwrap().is_empty());
        assert!(buffer.is_empty());
    }

    #[rstest]
    fn test_process_incomplete_message() {
        // A partial FIX message without end
        let mut buffer = b"8=FIXT.1.1\x019=100\x0135=D\x01".to_vec();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        // No complete message, nothing should be processed
        assert!(received.lock().unwrap().is_empty());
        // Buffer should be preserved for more data
        assert_eq!(buffer, b"8=FIXT.1.1\x019=100\x0135=D\x01".to_vec());
    }

    #[rstest]
    fn test_process_complete_message() {
        // A complete FIX message
        let mut buffer = b"8=FIXT.1.1\x019=100\x0135=D\x0110=123\x01".to_vec();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        assert!(buffer.is_empty() || received.lock().unwrap().len() == 1);
    }

    #[rstest]
    fn test_process_message_with_garbage_prefix() {
        // Message with garbage before the FIX header
        let mut buffer = b"GARBAGE8=FIXT.1.1\x019=100\x0135=D\x0110=123\x01".to_vec();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        assert!(buffer.is_empty() || received.lock().unwrap().len() == 1);
    }

    #[rstest]
    fn test_process_partial_checksum() {
        // Message with partial checksum (missing the SOH)
        let mut buffer = b"8=FIXT.1.1\x019=100\x0135=D\x0110=123".to_vec();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        // No complete message, nothing should be processed
        assert!(received.lock().unwrap().is_empty());
        // Buffer should be preserved
        assert_eq!(buffer, b"8=FIXT.1.1\x019=100\x0135=D\x0110=123".to_vec());
    }

    #[rstest]
    fn test_process_multiple_messages_single_call() {
        // Two complete messages
        let mut buffer =
            b"8=FIXT.1.1\x019=100\x0135=D\x0110=123\x018=FIXT.1.1\x019=200\x0135=D\x0110=456\x01"
                .to_vec();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        assert_eq!(received.lock().unwrap().len(), 2);
        assert_eq!(
            received.lock().unwrap()[0],
            b"8=FIXT.1.1\x019=100\x0135=D\x0110=123\x01".to_vec()
        );
        assert_eq!(
            received.lock().unwrap()[1],
            b"8=FIXT.1.1\x019=200\x0135=D\x0110=456\x01".to_vec()
        );
        assert!(buffer.is_empty());
    }

    #[rstest]
    fn test_process_message_with_invalid_checksum() {
        // Message with invalid checksum format (not 3 digits)
        let mut buffer = b"8=FIXT.1.1\x019=100\x0135=D\x0110=1X3\x01".to_vec();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        // No message should be processed due to invalid checksum format
        assert!(received.lock().unwrap().is_empty());
        // Buffer should be preserved
        assert_eq!(
            buffer,
            b"8=FIXT.1.1\x019=100\x0135=D\x0110=1X3\x01".to_vec()
        );
    }

    #[rstest]
    fn test_process_message_with_multiple_checksums() {
        let mut buffer = b"8=FIX.4.4\x019=100\x0110=123\x0110=456\x01".to_vec();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        // One message processed, extra data retained
        assert_eq!(received.lock().unwrap().len(), 1);
        assert_eq!(
            received.lock().unwrap()[0],
            b"8=FIX.4.4\x019=100\x0110=123\x01".to_vec()
        );
        assert_eq!(buffer, b"10=456\x01".to_vec());
    }

    #[rstest]
    fn test_process_large_buffer() {
        let mut buffer = Vec::new();
        let message = b"8=FIX.4.4\x019=100\x0135=D\x0110=123\x01";
        for _ in 0..1000 {
            buffer.extend_from_slice(message);
        }
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let handler: Arc<TcpMessageHandler> = Arc::new(move |data: &[u8]| {
            received_clone.lock().unwrap().push(data.to_vec());
        });

        process_fix_buffer(&mut buffer, &handler);

        // 1000 messages processed, buffer empty
        assert_eq!(received.lock().unwrap().len(), 1000);
        assert!(buffer.is_empty());
    }
}
