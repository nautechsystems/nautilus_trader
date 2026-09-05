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
    fn test_classify_okx_http_permanent_venue_error_is_rejected() {
        let error = OKXHttpError::OkxError {
            error_code: "51000".to_string(),
            message: "Parameter state error".to_string(),
        };
        let reason = error.to_string();

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::VenueRejected(reason)
        );
    }

    #[rstest]
    fn test_classify_okx_http_retryable_venue_error_is_ambiguous() {
        let error = OKXHttpError::RetryableOkxError {
            error_code: "50013".to_string(),
            message: "System busy, please try again later".to_string(),
            retry_after: None,
        };
        let reason = error.to_string();

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(reason)
        );
    }

    #[rstest]
    fn test_classify_okx_http_missing_credentials_is_not_sent() {
        let error = OKXHttpError::MissingCredentials;

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_validation_is_not_sent() {
        let error = OKXHttpError::ValidationError("invalid quantity".to_string());

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_invalid_retry_config_is_not_sent() {
        let error = OKXHttpError::ValidationError("invalid retry configuration".to_string());

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_timeout_is_ambiguous() {
        let error = OKXHttpError::OperationTimeout { timeout_ms: 1_000 };

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_budget_is_ambiguous() {
        let error = OKXHttpError::RetryBudgetExceeded("budget exceeded".to_string());

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_network_is_ambiguous() {
        let error = OKXHttpError::HttpClientError(HttpClientError::TransportError(
            "connection reset".to_string(),
        ));

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_permanent_client_error_is_ambiguous() {
        let error = OKXHttpError::HttpClientError(HttpClientError::Error(
            "response body exceeds maximum".to_string(),
        ));

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_unexpected_status_is_ambiguous() {
        let error = OKXHttpError::UnexpectedStatus {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: String::new(),
        };

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_response_decoding_is_ambiguous() {
        let error = OKXHttpError::ResponseDecoding("failed to deserialize".to_string());

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_request_serialization_is_not_sent() {
        let error = OKXHttpError::RequestSerialization("failed to serialize".to_string());

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_http_empty_response_is_ambiguous() {
        let error = OKXHttpError::EmptyResponse;

        assert_eq!(
            classify_okx_http_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_ws_handler_unavailable_is_not_sent() {
        let error = OKXWsError::HandlerUnavailable("channel closed".to_string());

        assert_eq!(
            classify_okx_ws_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_ws_no_active_client_is_not_sent() {
        let error = OKXWsError::NoActiveClient;

        assert_eq!(
            classify_okx_ws_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_ws_json_encode_is_not_sent() {
        let error = OKXWsError::JsonError("Failed to serialize order: eof".to_string());

        assert_eq!(
            classify_okx_ws_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_ws_send_failed_is_ambiguous() {
        let error = OKXWsError::SendFailed("connection reset".to_string());

        assert_eq!(
            classify_okx_ws_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_ws_pre_write_timeout_is_not_sent() {
        let error = OKXWsError::TransportSend(SendError::Timeout);

        assert_eq!(
            classify_okx_ws_failure(&error),
            CommandFailure::NotSent(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_ws_write_timeout_is_ambiguous() {
        let error = OKXWsError::TransportSend(SendError::WriteTimeout);

        assert_eq!(
            classify_okx_ws_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }

    #[rstest]
    fn test_classify_okx_ws_timeout_is_ambiguous() {
        let error = OKXWsError::OperationTimeout { timeout_ms: 2_000 };

        assert_eq!(
            classify_okx_ws_failure(&error),
            CommandFailure::Ambiguous(error.to_string())
        );
    }
}
