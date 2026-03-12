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
    assert "python ops/scripts/tokenmm_risk_audit.py" in runbook
    assert "python scripts/ops/tokenmm_risk_audit.py" not in runbook

    assert "docs/runbooks/tokenmm-risk-validation.md" in deploy_readme
    assert "docs/runbooks/tokenmm-risk-validation.md" in strategies_readme
    assert "docs/runbooks/tokenmm-risk-validation.md" in contract_doc


def test_tokenmm_binance_spot_docs_keep_the_strategy_parked_on_this_pass() -> None:
    runbook = _read(_repo_root() / "docs/runbooks/tokenmm-binance-spot-market-making.md")
    deploy_readme = _read(_repo_root() / "deploy/tokenmm/README.md")

    assert "docs/runbooks/tokenmm-binance-spot-market-making.md" in deploy_readme
    assert "Binance perp and Binance spot stay allowlisted but parked" in deploy_readme
    assert "supported live core or required completeness" in deploy_readme
    assert "`bot_on = false`" in deploy_readme
    assert "future reintroduction work" in deploy_readme

    assert "bot-off restart and canary" in runbook
    assert "terminal_order_denied" in runbook
    assert "USDT +1285.28070703" not in runbook
    assert "PLUME -30314.96734613" not in runbook
    assert "USDT +1285.28070703" not in deploy_readme
    assert "PLUME -30314.96734613" not in deploy_readme


def test_tokenmm_risk_audit_script_checks_canonical_endpoints_and_reconciliation_failures() -> (
    None
):
    canonical_script = _repo_root() / "ops/scripts/tokenmm_risk_audit.py"
    compatibility_shim = _repo_root() / "scripts/ops/tokenmm_risk_audit.py"
    script = _read(canonical_script)

    assert canonical_script.is_file()
    assert compatibility_shim.is_symlink()
    assert compatibility_shim.resolve() == canonical_script.resolve()

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


def test_tokenmm_docs_lock_snapshot_freshness_shared_inventory_publishers_and_backend_risk_groups() -> (
    None
):
    source_of_truth = _read(_repo_root() / "docs/architecture/tokenmm-risk-source-of-truth.md")
    portfolio_semantics = _read(
        _repo_root() / "docs/architecture/tokenmm-portfolio-inventory-semantics.md",
    )
    runbook = _read(_repo_root() / "docs/runbooks/tokenmm-risk-validation.md")
    deploy_readme = _read(_repo_root() / "deploy/tokenmm/README.md")
    contract_doc = _read(_repo_root() / "fluxboard/docs/tokenmm_contract.md")
    socket_doc = _read(_repo_root() / "fluxboard/docs/tokenmm_socket_contract.md")

    assert "stale_after_ms" in portfolio_semantics
    assert "fresh enough" in portfolio_semantics
    assert "MakerV3 and MakerV4" in portfolio_semantics
    assert "risk_groups" in source_of_truth
    assert "risk_key" in source_of_truth

    assert "stale_after_ms" in runbook
    assert "source = \"portfolio_snapshot\"" in runbook
    assert "falls back to the live per-strategy merge path" in runbook

    assert "stale_after_ms" in deploy_readme
    assert "risk_groups" in deploy_readme
    assert "risk_key" in deploy_readme

    assert "stale_after_ms" in contract_doc
    assert "risk_groups" in contract_doc
    assert "risk_groups[].rows" in contract_doc
    assert "risk_key" in contract_doc

    assert "risk_groups" in socket_doc
    assert "risk_key" in socket_doc
