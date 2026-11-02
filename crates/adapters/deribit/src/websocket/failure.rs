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

//! Deribit mapping from typed command evidence to [`CommandFailure`].
//!
//! Classification happens once from the error the WebSocket layer preserved. Retryability stays on
//! the transport error and is not reused as the outcome class. Strategy-facing reasons are derived
//! here and do not replace the typed error the caller logs.

use nautilus_live::execution::failure::CommandFailure;
use nautilus_network::error::SendError;

use super::error::DeribitWsError;
use crate::common::consts::DERIBIT_POST_ONLY_ERROR_CODE;

const MAX_REASON_CHARS: usize = 256;
const FALLBACK_REASON: &str = "Order command rejected";
const REASON_NOT_AUTHENTICATED: &str = "not authenticated";
const REASON_ADMISSION_CLOSED: &str = "command channel closed";
const REASON_NOT_CONNECTED: &str = "not connected";
const REASON_ENCODE: &str = "cannot encode order command";
pub(super) const REASON_CANCELED: &str = "canceled before transmission";
const REASON_NOT_SENT: &str = "not sent";
pub(crate) const REASON_NOT_STARTED: &str = "execution task was not started";

/// Venue codes that do not prove this command was unapplied.
///
/// `0` is documented as success. Rate limits follow the shared command policy and stay unknown.
/// `10040` asks for a retry and is not an explicit rejection. Internal, timeout, and mixed
/// failure codes do not prove the matching engine left the command unapplied.
const AMBIGUOUS_VENUE_CODES: [i64; 10] = [
    0, 10001, 10028, 10040, 10066, 11031, 11094, 13008, 13028, 13888,
];

/// Classifies an order-command failure that occurred before a venue response.
#[must_use]
pub(crate) fn classify_deribit_command_failure(error: &DeribitWsError) -> CommandFailure {
    match error {
        DeribitWsError::Authentication(_) => CommandFailure::not_sent(REASON_NOT_AUTHENTICATED),
        DeribitWsError::AdmissionClosed => CommandFailure::not_sent(REASON_ADMISSION_CLOSED),
        DeribitWsError::NotConnected => CommandFailure::not_sent(REASON_NOT_CONNECTED),
        DeribitWsError::Encode(_) => CommandFailure::not_sent(REASON_ENCODE),
        DeribitWsError::CanceledBeforeSend => CommandFailure::not_sent(REASON_CANCELED),
        DeribitWsError::TransportSend(
            SendError::InvalidInput(_)
            | SendError::BufferFull
            | SendError::Closed
            | SendError::Timeout
            | SendError::ConnectionChanged,
        ) => CommandFailure::not_sent(REASON_NOT_SENT),
        DeribitWsError::TransportSend(_) => CommandFailure::ambiguous(error.to_string()),
        _ => CommandFailure::ambiguous(error.to_string()),
    }
}

/// Classifies a correlated Deribit JSON-RPC error for one order command.
///
/// The boolean is `due_post_only` and is true only for code 11054.
#[must_use]
pub(crate) fn classify_deribit_venue_error(code: i64, message: &str) -> (CommandFailure, bool) {
    if AMBIGUOUS_VENUE_CODES.contains(&code) {
        return (
            CommandFailure::ambiguous(sanitize_strategy_reason(message)),
            false,
        );
    }

    (
        CommandFailure::venue_rejected(sanitize_strategy_reason(message)),
        code == DERIBIT_POST_ONLY_ERROR_CODE,
    )
}

/// Returns the strategy-facing reason when this evidence may emit a terminal event.
#[must_use]
pub(crate) fn terminal_reason(failure: &CommandFailure) -> Option<&str> {
    match failure {
        CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => Some(reason),
        CommandFailure::Ambiguous(_) => None,
    }
}

pub(crate) fn sanitize_strategy_reason(message: &str) -> String {
    let mut output = String::new();
    let mut length = 0;

    for character in message.chars() {
        let character = if is_stripped(character) {
            ' '
        } else {
            character
        };

        let previous_size = output.len();
        if character.is_whitespace() {
            if !output.is_empty() && !output.ends_with(' ') {
                output.push(' ');
            }
        } else {
            output.push(character);
        }

        length += usize::from(output.len() != previous_size);
        if length >= MAX_REASON_CHARS {
            break;
        }
    }

    let output = output.trim_end();
    if output.is_empty() {
        FALLBACK_REASON.to_string()
    } else {
        output.to_string()
    }
}

fn is_stripped(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00ad}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
        )
}

#[cfg(test)]
mod tests {
    use nautilus_network::error::SendError;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(DeribitWsError::Authentication("session missing".to_string()), "not authenticated")]
    #[case(DeribitWsError::AdmissionClosed, "command channel closed")]
    #[case(DeribitWsError::NotConnected, "not connected")]
    #[case(DeribitWsError::Encode("bad decimal".to_string()), "cannot encode order command")]
    #[case(DeribitWsError::CanceledBeforeSend, "canceled before transmission")]
    #[case(DeribitWsError::TransportSend(SendError::Closed), "not sent")]
    #[case(DeribitWsError::TransportSend(SendError::Timeout), "not sent")]
    #[case(DeribitWsError::TransportSend(SendError::BufferFull), "not sent")]
    #[case(
        DeribitWsError::TransportSend(SendError::ConnectionChanged),
        "not sent"
    )]
    fn test_pre_write_failure_is_not_sent(#[case] error: DeribitWsError, #[case] reason: &str) {
        assert_eq!(
            classify_deribit_command_failure(&error),
            CommandFailure::not_sent(reason)
        );
    }

    #[rstest]
    #[case(DeribitWsError::TransportSend(SendError::WriteTimeout))]
    #[case(DeribitWsError::TransportSend(SendError::BrokenPipe(
        "writer channel closed".to_string()
    )))]
    #[case(DeribitWsError::Timeout("response".to_string()))]
    #[case(DeribitWsError::ParsingError("bad payload".to_string()))]
    fn test_uncertain_transport_failure_is_ambiguous(#[case] error: DeribitWsError) {
        assert!(matches!(
            classify_deribit_command_failure(&error),
            CommandFailure::Ambiguous(_)
        ));
    }

    #[rstest]
    #[case(11054, "post_only_reject", true, true)]
    #[case(10044, "trigger_price_wrong_tick", true, false)]
    #[case(10004, "order_not_found", true, false)]
    #[case(10028, "too_many_requests", false, false)]
    #[case(10040, "retry", false, false)]
    #[case(11094, "internal_server_error", false, false)]
    #[case(13888, "timed_out", false, false)]
    #[case(0, "ok", false, false)]
    fn test_classify_deribit_venue_error(
        #[case] code: i64,
        #[case] message: &str,
        #[case] venue_rejected: bool,
        #[case] due_post_only: bool,
    ) {
        let (failure, post_only) = classify_deribit_venue_error(code, message);

        assert_eq!(post_only, due_post_only);

        if venue_rejected {
            assert_eq!(failure, CommandFailure::venue_rejected(message));
        } else {
            assert!(matches!(failure, CommandFailure::Ambiguous(_)));
            assert!(!post_only);
        }
    }

    #[rstest]
    fn test_sanitize_strategy_reason_strips_and_falls_back() {
        assert_eq!(
            sanitize_strategy_reason("bad\u{0000}\npost_only_reject"),
            "bad post_only_reject"
        );
        assert_eq!(sanitize_strategy_reason("\u{200b}\n\t"), FALLBACK_REASON);
        assert_eq!(
            sanitize_strategy_reason(&"x".repeat(MAX_REASON_CHARS + 20))
                .chars()
                .count(),
            MAX_REASON_CHARS
        );
    }
}
