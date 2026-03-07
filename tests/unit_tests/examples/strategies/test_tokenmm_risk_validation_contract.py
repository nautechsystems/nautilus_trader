from __future__ import annotations

from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_tokenmm_risk_validation_runbook_documents_authoritative_checks_and_rollout_gates() -> (
    None
):
    runbook = _read(_repo_root() / "docs/runbooks/tokenmm-risk-validation.md")
    deploy_readme = _read(_repo_root() / "deploy/tokenmm/README.md")
    strategies_readme = _read(_repo_root() / "deploy/tokenmm/strategies/README.md")
    contract_doc = _read(_repo_root() / "fluxboard/docs/tokenmm_contract.md")

    assert "local risk" in runbook
    assert "global risk" in runbook
    assert "/api/v1/signals?profile=tokenmm" in runbook
    assert "/api/v1/balances?profile=tokenmm" in runbook
    assert "/api/v1/balances?strategy=" in runbook
    assert "/api/pulse/jobs" in runbook
    assert "degraded metadata" in runbook
    assert "data unavailable" in runbook
    assert "true zero" in runbook
    assert "before enabling trading" in runbook
    assert "all targeted unit tests green" in runbook
    assert "TokenMM group restarted cleanly through Pulse" in runbook
    assert "partial vs strict" in runbook
    assert "startup reconciliation" in runbook

    assert "docs/runbooks/tokenmm-risk-validation.md" in deploy_readme
    assert "docs/runbooks/tokenmm-risk-validation.md" in strategies_readme
    assert "docs/runbooks/tokenmm-risk-validation.md" in contract_doc


def test_tokenmm_risk_audit_script_checks_canonical_endpoints_and_reconciliation_failures() -> (
    None
):
    script = _read(_repo_root() / "scripts/ops/tokenmm_risk_audit.py")

    assert "/api/v1/signals?profile=tokenmm" in script
    assert "/api/v1/balances?profile=tokenmm" in script
    assert "/api/v1/balances?strategy=" in script
    assert "/api/pulse/jobs" in script
    assert "global_qty_base" in script
    assert "missing_required" in script
    assert "stale_required" in script
    assert "null_qty_required" in script
    assert "blocked_reconciliation" in script
    assert "local qty" in script.lower()
    assert "argparse" in script
    assert "__main__" in script
