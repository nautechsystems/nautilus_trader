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
Test the version-neutral BacktestEngine benchmark driver.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest


WORKSPACE_ROOT = Path(__file__).resolve().parents[3]


def _load_benchmark_module() -> object:
    module_path = WORKSPACE_ROOT / "scripts" / "benchmark-backtest-versions.py"
    module_name = "benchmark_backtest_versions"
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


benchmark = _load_benchmark_module()


def test_scenario_matrix_covers_required_dimensions() -> None:
    """
    Test the bounded matrix retains every comparison dimension.
    """
    scenarios = benchmark.SCENARIOS

    assert len(scenarios) == 16
    assert {scenario.fixture for scenario in scenarios} == {
        "bars_bid_ask",
        "bars_last",
        "depth10",
        "l2_deltas",
        "quotes_trades",
        "trigger_quotes",
    }
    assert {scenario.size for scenario in scenarios} == {"small", "medium", "large"}
    assert {scenario.strategy for scenario in scenarios} == {
        "accumulating_market",
        "alternating_market",
        "bar_audit",
        "gtd_expiry",
        "none",
        "order_type_sweep",
        "passive_cancel",
    }
    assert {scenario.book_type for scenario in scenarios} == {"L1_MBP", "L2_MBP"}
    assert any(scenario.streams > 1 for scenario in scenarios)
    assert any(scenario.instruments > 1 for scenario in scenarios)
    assert any(scenario.queue_position for scenario in scenarios)
    assert any(scenario.liquidity_consumption for scenario in scenarios)
    assert [
        (scenario.name, scenario.count)
        for scenario in scenarios
        if scenario.strategy == "accumulating_market"
    ] == [
        ("accumulating_market_small", 250),
        ("accumulating_market_medium", 1_000),
        ("accumulating_market_large", 4_000),
    ]


def test_ordered_cases_runs_each_case_once_per_session() -> None:
    """
    Test session interleaving changes order without changing coverage.
    """
    expected = {
        (scenario.name, boundary)
        for scenario in benchmark.SCENARIOS
        for boundary in benchmark.BOUNDARIES
    }

    sessions = [benchmark.ordered_cases(index) for index in range(3)]

    assert all(len(session) == len(expected) for session in sessions)
    assert all(
        {(case.name, boundary) for case, boundary in session} == expected for session in sessions
    )
    assert sessions[0] != sessions[1]
    assert sessions[1] != sessions[2]


def test_summarize_reports_median_spread_ratio_and_gap() -> None:
    """
    Test summary statistics use raw elapsed samples without profile data.
    """
    samples = []
    for version, values in {"v1": [10, 20, 30], "v2": [30, 40, 50]}.items():
        samples.extend(
            {
                "boundary": "run_preloaded",
                "elapsed_ns": value,
                "scenario": "replay",
                "version": version,
            }
            for value in values
        )

    result = benchmark.summarize(samples)

    assert result == [
        {
            "boundary": "run_preloaded",
            "scenario": "replay",
            "v1_max_ns": 30,
            "v1_median_ns": 20,
            "v1_min_ns": 10,
            "v1_spread_percent": 100.0,
            "v1_spread_ns": 20,
            "v2_max_ns": 50,
            "v2_median_ns": 40,
            "v2_min_ns": 30,
            "v2_spread_percent": 50.0,
            "v2_spread_ns": 20,
            "v2_v1_gap_ns": 20,
            "v2_v1_gap_percent": 100.0,
            "v2_v1_ratio": 2.0,
        },
    ]


def test_require_fingerprint_match_rejects_mutated_state() -> None:
    """
    Test a changed semantic component stops a timed comparison.
    """
    expected = {
        "event_counts": {"iterations": 10},
        "digests": {"result_digest": "sha256:expected"},
    }
    mutated = {
        "event_counts": {"iterations": 9},
        "digests": {"result_digest": "sha256:expected"},
    }

    with pytest.raises(RuntimeError, match="Fingerprint mismatch for mutation test"):
        benchmark.require_fingerprint_match(expected, mutated, "mutation test")


def test_stable_decimal_uses_domain_precision() -> None:
    """
    Test legacy float noise does not change an exact domain price.
    """
    assert benchmark.stable_decimal(49_989.049999999996, 2) == "49989.05"
    assert benchmark.stable_decimal(None, 2) is None


def test_file_contains_matches_across_read_blocks(tmp_path: Path) -> None:
    """
    Test embedded build identity can span file read boundaries.
    """
    path = tmp_path / "extension.so"
    path.write_bytes(b"x" * (1024 * 1024 - 3) + b"exact-commit")

    assert benchmark.file_contains(path, b"exact-commit")
    assert not benchmark.file_contains(path, b"other-commit")


def test_untracked_files_sha256_changes_with_file_contents(tmp_path: Path) -> None:
    """
    Test source identity binds untracked paths to their exact contents.
    """
    path = tmp_path / "nested" / "source.py"
    path.parent.mkdir()
    path.write_text("alpha", encoding="utf-8")
    paths = benchmark.os.fsencode(path.relative_to(tmp_path)) + b"\0"

    initial = benchmark.untracked_files_sha256(tmp_path, paths)
    path.write_text("bravo", encoding="utf-8")

    assert benchmark.untracked_files_sha256(tmp_path, paths) != initial


def test_require_installed_artifact_rejects_different_wheel(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """
    Test runtime identity stays bound to the wheel installed in the environment.
    """
    artifact = tmp_path / "runtime.whl"
    artifact.write_bytes(b"wheel")
    metadata = {"url": artifact.resolve().as_uri()}

    class Distribution:
        def read_text(self, filename: str) -> str:
            assert filename == "direct_url.json"
            return benchmark.json.dumps(metadata)

    monkeypatch.setattr(benchmark.importlib.metadata, "distribution", lambda _name: Distribution())

    assert benchmark.require_installed_artifact(artifact) == artifact.resolve().as_uri()

    metadata["url"] = (tmp_path / "different.whl").resolve().as_uri()
    with pytest.raises(RuntimeError, match="Installed distribution came from"):
        benchmark.require_installed_artifact(artifact)


def test_require_wheel_extension_rejects_modified_install(tmp_path: Path) -> None:
    """
    Test runtime identity binds the loaded extension bytes to the recorded wheel.
    """
    artifact = tmp_path / "runtime.whl"
    extension = (
        tmp_path
        / "nautilus_trader"
        / ".venv"
        / "lib"
        / "python3.12"
        / "site-packages"
        / "nautilus_trader"
        / "runtime.so"
    )
    extension.parent.mkdir(parents=True)
    extension.write_bytes(b"release extension")
    with benchmark.zipfile.ZipFile(artifact, "w") as wheel:
        wheel.writestr("nautilus_trader/runtime.so", b"release extension")

    assert benchmark.require_wheel_extension(artifact, extension) == (
        "nautilus_trader/runtime.so",
        benchmark.file_sha256(extension),
    )

    extension.write_bytes(b"modified extension")
    with pytest.raises(RuntimeError, match="does not match the extension stored in the wheel"):
        benchmark.require_wheel_extension(artifact, extension)


def test_require_identity_match_rejects_changed_worker() -> None:
    """
    Test a timed worker cannot report a changed runtime identity.
    """
    expected = {
        "artifact_url": "file:///runtime.whl",
        "extension_sha256": "release",
        "generation": "v2",
        "package_version": "2.0.0rc4",
    }
    changed = {**expected, "extension_sha256": "modified"}

    with pytest.raises(RuntimeError, match="Worker identity field extension_sha256"):
        benchmark.require_identity_match(expected, changed, "Worker identity")


def test_run_worker_records_and_revalidates_each_sample_identity(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """
    Test every timed sample is followed by a complete identity check.
    """
    identity = {
        "artifact_sha256": "wheel",
        "extension_sha256": "extension",
        "source_commit": "commit",
        "source_status_sha256": "status",
        "wheel_extension_sha256": "extension",
    }
    fingerprint = {"digests": {"result_digest": "sha256:result"}}
    args = benchmark.argparse.Namespace(
        scenario="quote_trade_replay_small",
        boundary="run_preloaded",
        iterations=1,
        expected_identity_digest=benchmark.digest(identity),
    )
    identities = iter([identity])
    monkeypatch.setattr(benchmark, "load_bindings", object)
    monkeypatch.setattr(benchmark, "runtime_identity", lambda _args: next(identities))
    monkeypatch.setattr(
        benchmark,
        "run_iteration",
        lambda *_args: {"elapsed_ns": 1, "fingerprint": fingerprint},
    )

    benchmark.run_worker(args)

    output = benchmark.json.loads(capsys.readouterr().out)
    assert output["samples"][0]["runtime_identity"] == identity

    changed = {**identity, "source_commit": "changed"}
    identities = iter([changed])
    with pytest.raises(RuntimeError, match=r"Identity mismatch for .* timed iteration 0"):
        benchmark.run_worker(args)
