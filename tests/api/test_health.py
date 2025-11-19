import contextlib
import os
import subprocess
import sys
import time

import httpx
import pytest


pytestmark = pytest.mark.smoke


def _start_api():
    # Start uvicorn in background for the duration of this test module
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
    # Wait for readiness
    for _ in range(60):
        try:
            r = httpx.get("http://127.0.0.1:8000/health", timeout=1.0)
            if r.status_code == 200:
                return proc
        except Exception:
            # Reduce flakiness during local CI; next iteration retries
            time.sleep(0.5)
            continue
        time.sleep(0.5)
    # If not ready, terminate and fail
    with contextlib.suppress(Exception):
        proc.terminate()
    raise RuntimeError("API did not become ready")


@pytest.fixture(scope="module")
def api_proc():
    proc = _start_api()
    yield proc
    with contextlib.suppress(Exception):
        proc.terminate()


def test_health(api_proc):
    r = httpx.get("http://127.0.0.1:8000/health", timeout=3.0)
    assert r.status_code == 200
    assert r.json().get("status") == "ok"
