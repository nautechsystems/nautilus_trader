from __future__ import annotations

from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(relative_path: str) -> str:
    return (_repo_root() / relative_path).read_text(encoding="utf-8")


def test_lp_prod_runbook_documents_shared_host_topology() -> None:
    text = _read("docs/runbooks/lp-hedger-production-rollout.md")

    assert "/lp" in text
    assert "/api/v1/hedgers/*" in text
    assert "LP_API_BACKEND_URL=http://127.0.0.1:5025" in text
    assert "flux-lp.target" in text
    assert "service-eth-plume-lp-hedger" in text
    assert "service-eth-plume-lp-hedger-band2" in text
    assert "rollback" in text.lower()


def test_lp_prod_docs_keep_band1_band2_as_only_active_instances() -> None:
    text = _read("deploy/lp/README.md")

    assert "Band1 and Band2" in text
    assert ".ini.disabled" in text
