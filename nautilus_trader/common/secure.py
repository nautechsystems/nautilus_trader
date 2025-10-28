# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Secure string handling for sensitive data like API keys and passwords.

This module provides utilities for safely storing and handling sensitive strings in
memory, with automatic clearing and safe redaction for logging.

"""

import secrets


class SecureString:
    """
    A secure wrapper for sensitive string data that provides automatic memory clearing
    and safe redaction for logging.

    This class helps prevent accidental exposure of sensitive data like
    API keys, passwords, and secrets in logs or debug output.

    Parameters
    ----------
    value : str
        The sensitive string value to protect.
    name : str, optional
        A descriptive name for this credential (e.g., "api_key", "password").
        Used in string representations.

    """

    def __init__(self, value: str, name: str = "credential") -> None:
        if not isinstance(value, str):
            raise TypeError(f"Value must be a string, was {type(value).__name__}")

        self._name = name
        self._value = value
        # Store as bytearray for easier memory clearing
        self._bytes = bytearray(value.encode("utf-8"))
        self._is_cleared = False

    def get_value(self) -> str:
        """
        Get the actual sensitive value.

        Returns
        -------
        str
            The sensitive string value.

        Raises
        ------
        ValueError
            If the value has been cleared.

        """
        if self._is_cleared:
            raise ValueError(f"{self._name} has been cleared from memory")
        return self._value

    def get_redacted(self, visible_chars: int = 4) -> str:
        """
        Get a redacted version of the value suitable for logging.

        Parameters
        ----------
        visible_chars : int, default 4
            Number of characters to show at start and end.

        Returns
        -------
        str
            Redacted string like "abcd...wxyz" or "<empty>" if value is empty.

        """
        if self._is_cleared:
            return f"<{self._name}:cleared>"

        if not self._value:
            return f"<{self._name}:empty>"

        value_len = len(self._value)

        if value_len <= visible_chars * 2:
            # Too short to meaningfully redact
            return f"<{self._name}:***>"

        start = self._value[:visible_chars]
        end = self._value[-visible_chars:]
        return f"{start}...{end}"

    def clear(self) -> None:
        """
        Clear the sensitive value from memory.

        This method overwrites the internal storage with random data before clearing to
        help prevent memory inspection attacks.

        """
        if not self._is_cleared:
            # Overwrite with random bytes
            random_bytes = secrets.token_bytes(len(self._bytes))
            for i in range(len(self._bytes)):
                self._bytes[i] = random_bytes[i]

            # Clear the references
            self._bytes.clear()
            self._value = ""
            self._is_cleared = True

    def __del__(self) -> None:
        """
        Clear sensitive data when object is garbage collected.
        """
        self.clear()

    def __str__(self) -> str:
        """
        Return a safely redacted string representation.
        """
        return self.get_redacted()

    def __repr__(self) -> str:
        """
        Return a safely redacted representation for debugging.
        """
        return f"SecureString(name='{self._name}', value={self.get_redacted()})"

    def __eq__(self, other: object) -> bool:
        """
        Compare with another SecureString or regular string.

        Parameters
        ----------
        other : Any
            Object to compare with.

        Returns
        -------
        bool
            True if values are equal.

        """
        if isinstance(other, SecureString):
            if self._is_cleared or other._is_cleared:
                return False
            return self._value == other._value
        elif isinstance(other, str):
            if self._is_cleared:
                return False
            return self._value == other
        return False

    def __bool__(self) -> bool:
        """
        Return True if the value is non-empty and not cleared.
        """
        return not self._is_cleared and bool(self._value)

    def __len__(self) -> int:
        """
        Return the length of the stored value.
        """
        if self._is_cleared:
            return 0
        return len(self._value)

    @classmethod
    def from_env(cls, env_var: str, name: str | None = None) -> "SecureString":
        """
        Create a SecureString from an environment variable.

        Parameters
        ----------
        env_var : str
            Name of the environment variable.
        name : str, optional
            Descriptive name for the credential.

        Returns
        -------
        SecureString
            The secure wrapper for the environment variable value.

        Raises
        ------
        ValueError
            If the environment variable is not set.

        """
        import os

        value = os.environ.get(env_var)
        if value is None:
            raise ValueError(f"Environment variable {env_var} is not set")

        return cls(value, name or env_var)


def mask_api_key(api_key: str, visible_chars: int = 4) -> str:
    """
    Mask an API key for safe logging.

    Parameters
    ----------
    api_key : str
        The API key to mask.
    visible_chars : int, default 4
        Number of characters to show at start and end.

    Returns
    -------
    str
        Masked API key like "abcd...wxyz".

    """
    if not api_key:
        return "<empty>"

    if len(api_key) <= visible_chars * 2:
        return "***"

    return f"{api_key[:visible_chars]}...{api_key[-visible_chars:]}"
