# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Test explicit session configuration and administration credential separation.
"""

import asyncio
import json
import time

import pytest

from nautilus_trader.adapters.polymarket import PolymarketExecutionClientConfig
from nautilus_trader.adapters.polymarket import PolymarketSessionKeyClient
from nautilus_trader.adapters.polymarket import PolymarketSessionKeyClientConfig
from nautilus_trader.adapters.polymarket import PolymarketSignatureType
from nautilus_trader.adapters.polymarket import PolymarketSignerType


PRIVATE_KEY = "0x" + "0" * 63 + "1"
WALLET = "0x1111111111111111111111111111111111111111"


def test_owner_signing_remains_default() -> None:
    """
    Owner configurations preserve the existing signing default.
    """
    assert PolymarketExecutionClientConfig().signer_type == PolymarketSignerType.Owner


@pytest.mark.parametrize(
    ("role", "value"),
    [(PolymarketSignerType.Owner, 0), (PolymarketSignerType.Session, 1)],
)
def test_signer_type_hash_matches_integer(role: PolymarketSignerType, value: int) -> None:
    """
    Integer-equal signer roles work interchangeably as mapping and set keys.
    """
    assert role == value
    assert hash(role) == hash(value)
    assert {role: "value"}[value] == "value"
    assert {value: "value"}[role] == "value"
    assert len({role, value}) == 1


@pytest.mark.parametrize(
    "missing",
    ["private_key", "api_key", "api_secret", "passphrase", "funder"],
)
def test_session_requires_explicit_credentials(missing: str) -> None:
    """
    Session configurations reject incomplete explicit credentials.
    """
    kwargs = {
        "signer_type": PolymarketSignerType.Session,
        "signature_type": PolymarketSignatureType.Poly1271,
        "private_key": PRIVATE_KEY,
        "api_key": "session-api-key",
        "api_secret": "c2Vzc2lvbg==",
        "passphrase": "session-passphrase",
        "funder": WALLET,
    }
    del kwargs[missing]
    with pytest.raises(ValueError, match="Session"):
        PolymarketExecutionClientConfig(**kwargs)


def test_session_configuration_selects_role() -> None:
    """
    Session configurations retain the selected role and wallet.
    """
    config = PolymarketExecutionClientConfig(
        signer_type=PolymarketSignerType.Session,
        signature_type=PolymarketSignatureType.Poly1271,
        private_key=PRIVATE_KEY,
        api_key="session-api-key",
        api_secret="c2Vzc2lvbg==",
        passphrase="session-passphrase",
        funder=WALLET,
    )
    assert config.signer_type == PolymarketSignerType.Session
    assert config.signature_type == PolymarketSignatureType.Poly1271
    assert config.funder == WALLET


@pytest.mark.parametrize("proxy_url", [None, "http://proxy-user:proxy-secret@localhost:8080"])
def test_admin_configuration_redacts_credentials(proxy_url: str | None) -> None:
    """
    Administration configuration representations omit every credential.
    """
    credentials = {
        "private_key": PRIVATE_KEY,
        "api_key": "owner-api-key",
        "api_secret": "b3duZXI=",
        "passphrase": "owner-passphrase",
        "builder_api_key": "builder-api-key",
        "builder_api_secret": "YnVpbGRlcg==",
        "builder_passphrase": "builder-passphrase",
    }
    config = PolymarketSessionKeyClientConfig(
        **credentials,
        funder=WALLET,
        base_url_http="https://clob.example.test",
        base_url_relayer="https://relayer.example.test",
        proxy_url=proxy_url,
    )
    representations = (repr(config), str(config))
    assert config.funder == WALLET
    assert config.base_url_http == "https://clob.example.test"
    assert config.base_url_relayer == "https://relayer.example.test"
    assert config.has_proxy_url is (proxy_url is not None)
    assert not hasattr(config, "proxy_url")
    for name, value in credentials.items():
        assert not hasattr(config, name)
        assert all(value not in representation for representation in representations)
    if proxy_url is not None:
        assert all(proxy_url not in representation for representation in representations)


@pytest.mark.asyncio
async def test_admin_lists_typed_metadata() -> None:
    """
    The async Python boundary preserves the complete session metadata.
    """
    address = "0x2222222222222222222222222222222222222222"
    requests = []
    valid_until = int(time.time()) + 86400

    async def respond(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        requests.append(await reader.readuntil(b"\r\n\r\n"))
        body = json.dumps(
            {
                "wallet": WALLET,
                "signers": [{"address": address, "scopes": ["CLOB"], "valid_until": valid_until}],
            },
        ).encode()
        writer.write(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
            + str(len(body)).encode()
            + b"\r\nConnection: close\r\n\r\n"
            + body,
        )
        await writer.drain()
        writer.close()
        await writer.wait_closed()

    server = await asyncio.start_server(respond, "127.0.0.1", 0)
    async with server:
        port = server.sockets[0].getsockname()[1]
        client = PolymarketSessionKeyClient(
            PolymarketSessionKeyClientConfig(
                private_key=PRIVATE_KEY,
                api_key="owner-api-key",
                api_secret="b3duZXI=",
                passphrase="owner-passphrase",
                builder_api_key="builder-api-key",
                builder_api_secret="YnVpbGRlcg==",
                builder_passphrase="builder-passphrase",
                funder=WALLET,
                base_url_http=f"http://127.0.0.1:{port}",
            ),
        )
        keys = await client.list_session_keys()
    assert [(key.address, key.scopes, key.valid_until) for key in keys] == [
        (address, ["CLOB"], valid_until),
    ]
    assert len(requests) == 1
    assert requests[0].startswith(b"GET /v1/user/session-signers HTTP/1.1\r\n")
