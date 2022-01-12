# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------


class BetfairError(Exception):
    """
    The base class for all `Betfair` specific errors.
    """

    pass


class BetfairAPIError(BetfairError):
    """
    Represents a `Betfair` API specific error.
    """

    def __init__(self, code: str, message: str):
        super().__init__()
        self.code = code
        self.message = message
        self.kind = ERROR_CODES.get(message, {}).get("kind")
        self.reason = ERROR_CODES.get(message, {}).get("reason")

    def __str__(self) -> str:
        return f"BetfairAPIError(code='{self.code}', message='{self.message}', kind='{self.kind}', reason='{self.reason}')"


ERROR_CODES = {
    "DSC-0008": {"kind": "JSONDeserialisationParseFailure", "reason": ""},
    "DSC-0009": {
        "kind": "ClassConversionFailure",
        "reason": "Invalid format for parameter, for example passing a string where a number was expected. "
        "Can also happen when a value is passed that does not match any valid enum.",
    },
    "DSC-0015": {
        "kind": "SecurityException",
        "reason": "Credentials supplied in request were invalid",
    },
    "DSC-0018": {
        "kind": "MandatoryNotDefined",
        "reason": "A parameter marked as mandatory was not provided",
    },
    "DSC-0019": {"kind": "Timeout", "reason": "The request has timed out"},
    "DSC-0021": {"kind": "NoSuchOperation", "reason": "The operation specified does not exist"},
    "DSC-0023": {"kind": "NoSuchService", "reason": ""},
    "DSC-0024": {
        "kind": "RescriptDeserialisationFailure",
        "reason": "Exception during deserialization of RESCRIPT request",
    },
    "DSC-0034": {
        "kind": "UnknownCaller",
        "reason": "A valid and active App Key hasn't been provided in the request. Please check that your App Key "
        "is active. Please see Application Keys for further information regarding App Keys.",
    },
    "DSC-0035": {"kind": "UnrecognisedCredentials", "reason": " "},
    "DSC-0036": {"kind": "InvalidCredentials", "reason": " "},
    "DSC-0037": {
        "kind": "SubscriptionRequired",
        "reason": "The user is not subscribed to the App Key provided",
    },
    "DSC-0038": {
        "kind": "OperationForbidden",
        "reason": "The App Key sent with the request is not permitted to access the operation",
    },
    "ANGX-0003": {
        "kind": "INVALID_SESSION_INFORMATION",
        "reason": "The session token hasn't been provided, is invalid or has expired. Login again to create a new session",
    },
    "AANGX-0004": {
        "kind": "InvalidAppKey",
        "reason": "The App Key (or password) is not valid",
    },
}
