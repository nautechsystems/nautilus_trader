"""Path-based strategy loader and resolved config tests."""

from __future__ import annotations

import json
import shutil
import textwrap
from pathlib import Path

import pytest

from polymarket.backtest_v1 import run_from_config


def test_path_strategy_loader_writes_resolved_config(tmp_path: Path) -> None:
    fixture_dir = Path("polymarket/tests/fixtures/strategy_loader_example")
    experiment_dir = tmp_path / "strategy_loader_example"
    experiment_dir.mkdir()
    shutil.copyfile(fixture_dir / "strategy.py", experiment_dir / "strategy.py")
    config = experiment_dir / "experiment.yml"
    config.write_text(
        textwrap.dedent(
            f"""
            experiment:
              name: strategy_loader_example
            adapter:
              name: live_ws_v1
              input:
                ndjson_path: {Path('polymarket/tests/fixtures/live_ws_minimal.ndjson').resolve().as_posix()}
            selection:
              asset_id: "yes"
            strategy:
              path: ./strategy.py
              class: LoaderExampleStrategy
              params:
                quantity: "5"
            runtime:
              run_id: strategy-loader-test
            report:
              output_dir: ./runs
            """,
        ).lstrip(),
        encoding="utf-8",
    )
    summary = run_from_config(config)
    resolved_path = Path(summary["outputs"]["resolved_config"])
    original_path = Path(summary["outputs"]["original_config"])
    resolved = json.loads(resolved_path.read_text(encoding="utf-8"))

    assert resolved["strategy"]["loader_mode"] == "path"
    assert resolved["strategy"]["source_path_original"] == "./strategy.py"
    assert resolved["strategy"]["source_path_resolved"] == (experiment_dir / "strategy.py").resolve().as_posix()
    assert resolved["strategy"]["class"] == "LoaderExampleStrategy"
    assert resolved["strategy"]["params_resolved"] == {"quantity": "5"}
    assert resolved["strategy"]["warnings"] == [
        "strategy.path resolves outside the repository; audit before sharing this run.",
    ]
    assert original_path.read_text(encoding="utf-8") == config.read_text(encoding="utf-8")
    assert summary["backtest"]["fills"] == 1
    assert summary["run_dir"] == (experiment_dir / "runs" / "strategy-loader-test").resolve().as_posix()


def test_report_output_dir_must_stay_inside_experiment_runs(tmp_path: Path) -> None:
    config = tmp_path / "bad-output.yml"
    config.write_text(
        textwrap.dedent(
            f"""
            experiment:
              name: bad_output
            adapter:
              name: live_ws_v1
              input:
                ndjson_path: {Path('polymarket/tests/fixtures/live_ws_minimal.ndjson').resolve().as_posix()}
            selection:
              asset_id: "yes"
            strategy:
              path: {Path('polymarket/tests/fixtures/strategy_loader_example/strategy.py').resolve().as_posix()}
              class: LoaderExampleStrategy
              params:
                quantity: "5"
            runtime:
              run_id: bad-output
            report:
              output_dir: ../outside-runs
            """,
        ).lstrip(),
        encoding="utf-8",
    )

    with pytest.raises(ValueError, match="experiment-local runs"):
        run_from_config(config)


def test_outside_repo_strategy_path_is_audited_in_resolved_config(tmp_path: Path) -> None:
    strategy = tmp_path / "external_strategy.py"
    strategy.write_text(
        textwrap.dedent(
            """
            from polymarket.strategies.base import BasePolymarketStrategyV1


            class ExternalStrategy(BasePolymarketStrategyV1):
                def on_replay_step(self, step, book, context):
                    pass
            """,
        ).lstrip(),
        encoding="utf-8",
    )
    config = tmp_path / "outside-strategy.yml"
    config.write_text(
        textwrap.dedent(
            f"""
            experiment:
              name: outside_strategy
            adapter:
              name: live_ws_v1
              input:
                ndjson_path: {Path('polymarket/tests/fixtures/live_ws_minimal.ndjson').resolve().as_posix()}
            selection:
              asset_id: "yes"
            strategy:
              path: {strategy.as_posix()}
              class: ExternalStrategy
            runtime:
              run_id: outside-strategy
            report:
              output_dir: ./runs
            """,
        ).lstrip(),
        encoding="utf-8",
    )

    summary = run_from_config(config)
    resolved = json.loads(Path(summary["outputs"]["resolved_config"]).read_text(encoding="utf-8"))

    assert resolved["strategy"]["warnings"] == [
        "strategy.path resolves outside the repository; audit before sharing this run.",
    ]
