import contextlib
import os
import subprocess
import sys
import time

import httpx
import pytest

from services.api.main import app


pytestmark = pytest.mark.smoke


@pytest.mark.skipif(os.getenv("E2E_UVICORN") == "1", reason="E2E mode uses uvicorn subprocess")
def test_health_inprocess():
    transport = httpx.ASGITransport(app=app)
    with httpx.Client(transport=transport, base_url="http://test") as client:
        r = client.get("/health", timeout=2.0)
        assert r.status_code == 200
        assert r.json().get("status") == "ok"


@pytest.mark.skipif(os.getenv("E2E_UVICORN") != "1", reason="Only run when explicitly enabled")
class TestHealthUvicorn:
    @staticmethod
    def _start_api():
        proc = subprocess.Popen(
            [
                sys.executable,
                "-m",
                "uvicorn",
                "services.api.main:app",
                "--host",
                "127.0.0.1",
                "--port",
                "8000",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env={**os.environ},
        )
        for _ in range(60):
            try:
                r = httpx.get("http://127.0.0.1:8000/health", timeout=1.0)
                if r.status_code == 200:
                    return proc
            except Exception:
                time.sleep(0.5)
                continue
            time.sleep(0.5)
        with contextlib.suppress(Exception):
            proc.terminate()
        raise RuntimeError("API did not become ready")

    @pytest.fixture(scope="class")
    def api_proc(self):
        proc = self._start_api()
        yield proc
        with contextlib.suppress(Exception):
            proc.terminate()

    def test_health(self, api_proc):
        r = httpx.get("http://127.0.0.1:8000/health", timeout=3.0)
        assert r.status_code == 200
        assert r.json().get("status") == "ok"
