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

//! OKX mapping from transport errors to [`CommandFailure`].

use nautilus_live::execution::failure::CommandFailure;
use nautilus_network::{error::SendError, http::HttpClientError};

use crate::{
    common::consts::{OKX_ORDER_REQUEST_TIMEOUT_CODE, should_retry_error_code},
    http::error::OKXHttpError,
    websocket::error::OKXWsError,
};

/// Classifies a structured OKX venue code.
///
/// Retryable system and rate-limit codes refuse this attempt but do not prove
/// the command was not applied.
#[must_use]
pub fn classify_okx_venue_code(error_code: &str, reason: impl Into<String>) -> CommandFailure {
    let reason = reason.into();

    if error_code.is_empty()
        || should_retry_error_code(error_code)
        || error_code == OKX_ORDER_REQUEST_TIMEOUT_CODE
    {
        CommandFailure::Ambiguous(reason)
    } else {
        CommandFailure::VenueRejected(reason)
    }
}

/// Classifies an OKX HTTP command failure from typed error evidence.
#[must_use]
pub fn classify_okx_http_failure(error: &OKXHttpError) -> CommandFailure {
    let reason = error.to_string();
    match error {
        OKXHttpError::MissingCredentials
        | OKXHttpError::ValidationError(_)
        | OKXHttpError::RequestSerialization(_)
        | OKXHttpError::HttpClientError(
            HttpClientError::InvalidProxy(_) | HttpClientError::ClientBuildError(_),
        ) => CommandFailure::NotSent(reason),
        OKXHttpError::OkxError { error_code, .. }
        | OKXHttpError::RetryableOkxError { error_code, .. } => {
            classify_okx_venue_code(error_code, reason)
        }
        OKXHttpError::MalformedResponse(_)
        | OKXHttpError::ResponseDecoding(_)
        | OKXHttpError::Canceled(_)
        | OKXHttpError::HttpClientError(_)
        | OKXHttpError::RetryableStatus { .. }
        | OKXHttpError::UnexpectedStatus { .. }
        | OKXHttpError::OperationTimeout { .. }
        | OKXHttpError::RetryBudgetExceeded(_)
        | OKXHttpError::EmptyResponse => CommandFailure::Ambiguous(reason),
    }
}

/// Classifies an OKX WebSocket command failure from typed error evidence.
#[must_use]
pub fn classify_okx_ws_failure(error: &OKXWsError) -> CommandFailure {
    let reason = error.to_string();
    match error {
        OKXWsError::ClientError(_)
        | OKXWsError::JsonError(_)
        | OKXWsError::NoActiveClient
        | OKXWsError::HandlerUnavailable(_)
        | OKXWsError::TransportSend(
            SendError::InvalidInput(_)
            | SendError::BufferFull
            | SendError::Closed
            | SendError::Timeout
            | SendError::ConnectionChanged,
        ) => CommandFailure::NotSent(reason),
        OKXWsError::OkxError { error_code, .. } => classify_okx_venue_code(error_code, reason),
        OKXWsError::ParsingError(_)
        | OKXWsError::AuthenticationError(_)
        | OKXWsError::TungsteniteError(_)
        | OKXWsError::TransportSend(SendError::WriteTimeout | SendError::BrokenPipe(_))
        | OKXWsError::SendFailed(_)
        | OKXWsError::OperationTimeout { .. } => CommandFailure::Ambiguous(reason),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_network::http::{HttpClientError, StatusCode};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::parameter_reject("51000", "Parameter state error", true)]
    #[case::system_busy("50013", "System busy, please retry later", false)]
    #[case::request_timeout("50004", "API endpoint request timeout", false)]
    #[case::order_timeout("51149", "Order timed out. Please try again.", false)]
    #[case::rate_limit("50011", "Request too frequent", false)]
    #[case::ws_rate_limit("60014", "WebSocket requests too frequent", true)]
    #[case::ws_internal_error("64007", "WebSocket internal error", true)]
    #[case::invalid_signature("50113", "Invalid signature", true)]
    #[case::missing_code("", "All operations failed", false)]
    fn test_classify_okx_venue_code(
        #[case] error_code: &str,
        #[case] message: &str,
        #[case] venue_rejected: bool,
    ) {
        let failure = classify_okx_venue_code(error_code, message);

        if venue_rejected {
            assert_eq!(failure, CommandFailure::VenueRejected(message.to_string()));
        } else {
            assert_eq!(failure, CommandFailure::Ambiguous(message.to_string()));
        }
    }

    #[rstest]
    #[case::permanent_venue_error(
        OKXHttpError::OkxError {
            error_code: "51000".to_string(),
            message: "Parameter state error".to_string(),
        },
        CommandFailure::VenueRejected
    )]
    #[case::retryable_venue_error(
        OKXHttpError::RetryableOkxError {
            error_code: "50013".to_string(),
            message: "System busy, please try again later".to_string(),
            retry_after: None,
        },
        CommandFailure::Ambiguous
    )]
    #[case::missing_credentials(OKXHttpError::MissingCredentials, CommandFailure::NotSent)]
    #[case::validation(
        OKXHttpError::ValidationError("invalid quantity".to_string()),
        CommandFailure::NotSent
    )]
    #[case::timeout(OKXHttpError::OperationTimeout { timeout_ms: 1_000 }, CommandFailure::Ambiguous)]
    #[case::budget(
        OKXHttpError::RetryBudgetExceeded("budget exceeded".to_string()),
        CommandFailure::Ambiguous
    )]
    #[case::network(
        OKXHttpError::HttpClientError(HttpClientError::TransportError(
            "connection reset".to_string(),
        )),
        CommandFailure::Ambiguous
    )]
    #[case::permanent_client_error(
        OKXHttpError::HttpClientError(HttpClientError::Error(
            "response body exceeds maximum".to_string(),
        )),
        CommandFailure::Ambiguous
    )]
    #[case::unexpected_status(
        OKXHttpError::UnexpectedStatus {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: String::new(),
        },
        CommandFailure::Ambiguous
    )]
    #[case::response_decoding(
        OKXHttpError::ResponseDecoding("failed to deserialize".to_string()),
        CommandFailure::Ambiguous
    )]
    #[case::request_serialization(
        OKXHttpError::RequestSerialization("failed to serialize".to_string()),
        CommandFailure::NotSent
    )]
    #[case::empty_response(OKXHttpError::EmptyResponse, CommandFailure::Ambiguous)]
    fn test_classify_okx_http_failure(
        #[case] error: OKXHttpError,
        #[case] expected: fn(String) -> CommandFailure,
    ) {
        let reason = error.to_string();
        assert_eq!(classify_okx_http_failure(&error), expected(reason));
    }

    #[rstest]
    #[case::handler_unavailable(
        OKXWsError::HandlerUnavailable("channel closed".to_string()),
        CommandFailure::NotSent
    )]
    #[case::no_active_client(OKXWsError::NoActiveClient, CommandFailure::NotSent)]
    #[case::json_encode(
        OKXWsError::JsonError("Failed to serialize order: eof".to_string()),
        CommandFailure::NotSent
    )]
    #[case::send_failed(
        OKXWsError::SendFailed("connection reset".to_string()),
        CommandFailure::Ambiguous
    )]
    #[case::pre_write_timeout(
        OKXWsError::TransportSend(SendError::Timeout),
        CommandFailure::NotSent
    )]
    #[case::pre_write_buffer_full(
        OKXWsError::TransportSend(SendError::BufferFull),
        CommandFailure::NotSent
    )]
    #[case::write_timeout(
        OKXWsError::TransportSend(SendError::WriteTimeout),
        CommandFailure::Ambiguous
    )]
    #[case::timeout(OKXWsError::OperationTimeout { timeout_ms: 2_000 }, CommandFailure::Ambiguous)]
    fn test_classify_okx_ws_failure(
        #[case] error: OKXWsError,
        #[case] expected: fn(String) -> CommandFailure,
    ) {
        let reason = error.to_string();
        assert_eq!(classify_okx_ws_failure(&error), expected(reason));
    }
}
