from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def test_markouts_docs_call_out_v0_scope_and_join_keys() -> None:
    runbook = (_repo_root() / "docs/runbooks/makerv3-markouts.md").read_text(encoding="utf-8")
    strategy_doc = (_repo_root() / "systems/flux/docs/makerv3.md").read_text(encoding="utf-8")

    assert "live-forward only" in runbook
    assert "flux:v1:trades:stream:{strategy_id}" in runbook
    assert "flux:v1:fv:stream:{strategy_id}" in runbook
    assert "execution_fill" in runbook
    assert "quote_cycle_id" in runbook
    assert "markouts_db_path" in runbook
    assert "deploy/tokenmm/tokenmm.live.toml" in runbook
    assert "raw live market-data history is out of scope" in runbook
    assert "markouts" in strategy_doc.lower()
