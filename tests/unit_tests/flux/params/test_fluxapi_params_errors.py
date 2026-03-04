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

from __future__ import annotations

from importlib.util import module_from_spec
from importlib.util import spec_from_file_location
from pathlib import Path
import sys

import pytest


MODULE_PATH = Path(__file__).resolve().parents[4] / "examples" / "live" / "poc" / "nautilus_fluxapi.py"
SPEC = spec_from_file_location("nautilus_fluxapi_for_params_tests", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
nautilus_fluxapi = module_from_spec(SPEC)
sys.modules[SPEC.name] = nautilus_fluxapi
SPEC.loader.exec_module(nautilus_fluxapi)


@pytest.fixture
def app_client():
    app = nautilus_fluxapi.build_app()
    with app.test_client() as client:
        yield client


def test_api_params_read_returns_explicit_error_when_params_store_invalid(monkeypatch, app_client) -> None:
    def _raise(*_args, **_kwargs):
        raise ValueError("Unknown params keys in flux:v1:params:bad: ['oops']")

    monkeypatch.setattr(nautilus_fluxapi, "_build_params_payload", _raise)

    response = app_client.get("/api/v1/params?strategy=bad")
    body = response.get_json()

    assert response.status_code == 500
    assert body == {
        "ok": False,
        "data": None,
        "error": {
            "code": "params_store_invalid",
            "message": "Unknown params keys in flux:v1:params:bad: ['oops']",
            "strategy_id": "bad",
        },
    }


def test_api_signals_read_returns_explicit_error_when_params_store_invalid(monkeypatch, app_client) -> None:
    def _raise(*_args, **_kwargs):
        raise ValueError("Unknown params keys in flux:v1:params:s1: ['oops']")

    monkeypatch.setattr(nautilus_fluxapi, "_build_signals_payload", _raise)

    response = app_client.get("/api/v1/signals?strategy=s1")
    body = response.get_json()

    assert response.status_code == 500
    assert body == {
        "ok": False,
        "data": None,
        "error": {
            "code": "params_store_invalid",
            "message": "Unknown params keys in flux:v1:params:s1: ['oops']",
            "strategy_id": "s1",
        },
    }
