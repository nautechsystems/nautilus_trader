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

import os

import pytest

from nautilus_trader.common.secure import SecureString
from nautilus_trader.common.secure import mask_api_key


class TestSecureString:
    """
    Tests for SecureString credential handling.
    """

    def test_init_with_valid_string(self):
        # Arrange, Act
        secure = SecureString("my_secret_key", name="api_key")

        # Assert
        assert secure.get_value() == "my_secret_key"
        assert len(secure) == 13
        assert bool(secure) is True

    def test_init_with_invalid_type_raises(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError, match="Value must be a string"):
            SecureString(12345)

    def test_get_redacted_with_long_string(self):
        # Arrange
        secure = SecureString("abcdefghijklmnopqrstuvwxyz", name="api_key")

        # Act
        redacted = secure.get_redacted()

        # Assert
        assert redacted == "abcd...wxyz"

    def test_get_redacted_with_custom_visible_chars(self):
        # Arrange
        secure = SecureString("abcdefghijklmnopqrstuvwxyz", name="api_key")

        # Act
        redacted = secure.get_redacted(visible_chars=2)

        # Assert
        assert redacted == "ab...yz"

    def test_get_redacted_with_short_string(self):
        # Arrange
        secure = SecureString("short", name="api_key")

        # Act
        redacted = secure.get_redacted()

        # Assert
        assert redacted == "<api_key:***>"

    def test_get_redacted_with_empty_string(self):
        # Arrange
        secure = SecureString("", name="api_key")

        # Act
        redacted = secure.get_redacted()

        # Assert
        assert redacted == "<api_key:empty>"

    def test_str_returns_redacted(self):
        # Arrange
        secure = SecureString("my_secret_key_12345", name="api_key")

        # Act
        result = str(secure)

        # Assert
        assert result == "my_s...2345"
        assert "secret" not in result

    def test_repr_returns_redacted(self):
        # Arrange
        secure = SecureString("my_secret_key_12345", name="api_key")

        # Act
        result = repr(secure)

        # Assert
        assert result == "SecureString(name='api_key', value=my_s...2345)"
        assert "secret" not in result

    def test_clear_removes_value(self):
        # Arrange
        secure = SecureString("my_secret_key", name="api_key")

        # Act
        secure.clear()

        # Assert
        with pytest.raises(ValueError, match="api_key has been cleared from memory"):
            secure.get_value()
        assert secure.get_redacted() == "<api_key:cleared>"
        assert len(secure) == 0
        assert bool(secure) is False

    def test_equality_with_another_secure_string(self):
        # Arrange
        secure1 = SecureString("my_secret", name="key1")
        secure2 = SecureString("my_secret", name="key2")
        secure3 = SecureString("different", name="key3")

        # Act, Assert
        assert secure1 == secure2
        assert secure1 != secure3

    def test_equality_with_string(self):
        # Arrange
        secure = SecureString("my_secret", name="key")

        # Act, Assert
        assert secure == "my_secret"
        assert secure != "different"

    def test_equality_after_clear(self):
        # Arrange
        secure1 = SecureString("my_secret", name="key1")
        secure2 = SecureString("my_secret", name="key2")

        # Act
        secure1.clear()

        # Assert
        assert secure1 != secure2
        assert secure1 != "my_secret"

    def test_from_env_with_existing_variable(self):
        # Arrange
        os.environ["TEST_API_KEY"] = "test_secret_token_xyz"

        # Act
        secure = SecureString.from_env("TEST_API_KEY")

        # Assert
        assert secure.get_value() == "test_secret_token_xyz"
        assert secure.get_redacted() == "test..._xyz"

        # Cleanup
        del os.environ["TEST_API_KEY"]

    def test_from_env_with_custom_name(self):
        # Arrange
        os.environ["TEST_API_KEY"] = "test_secret_token_xyz"

        # Act
        secure = SecureString.from_env("TEST_API_KEY", name="custom_key")

        # Assert
        assert secure.get_redacted() == "test..._xyz"
        assert "custom_key" in repr(secure)

        # Cleanup
        del os.environ["TEST_API_KEY"]

    def test_from_env_with_missing_variable_raises(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError, match="Environment variable MISSING_VAR is not set"):
            SecureString.from_env("MISSING_VAR")

    def test_bool_returns_false_for_empty_string(self):
        # Arrange
        secure = SecureString("", name="empty_key")

        # Act, Assert
        assert bool(secure) is False

    def test_bool_returns_false_after_clear(self):
        # Arrange
        secure = SecureString("value", name="key")

        # Act
        secure.clear()

        # Assert
        assert bool(secure) is False

    def test_multiple_clears_are_safe(self):
        # Arrange
        secure = SecureString("my_secret", name="key")

        # Act
        secure.clear()
        secure.clear()  # Should not raise

        # Assert
        assert secure.get_redacted() == "<key:cleared>"

    def test_memory_clearing_on_deletion(self):
        # Arrange
        secure = SecureString("my_secret_key", name="api_key")

        # Act - Force deletion
        del secure

        # Assert - Object is deleted, can't test directly but shouldn't crash


class TestMaskApiKey:
    """
    Tests for the mask_api_key utility function.
    """

    def test_mask_long_api_key(self):
        # Arrange
        api_key = "sk-1234567890abcdefghijklmnop"

        # Act
        masked = mask_api_key(api_key)

        # Assert
        assert masked == "sk-1...mnop"

    def test_mask_with_custom_visible_chars(self):
        # Arrange
        api_key = "sk-1234567890abcdefghijklmnop"

        # Act
        masked = mask_api_key(api_key, visible_chars=6)

        # Assert
        assert masked == "sk-123...klmnop"

    def test_mask_short_api_key(self):
        # Arrange
        api_key = "short"

        # Act
        masked = mask_api_key(api_key)

        # Assert
        assert masked == "***"

    def test_mask_empty_api_key(self):
        # Arrange
        api_key = ""

        # Act
        masked = mask_api_key(api_key)

        # Assert
        assert masked == "<empty>"

    def test_mask_none_api_key(self):
        # Arrange
        api_key = None

        # Act
        masked = mask_api_key(api_key)

        # Assert
        assert masked == "<empty>"
