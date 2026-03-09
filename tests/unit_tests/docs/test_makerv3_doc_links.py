from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_makerv3_refactor_docs_point_to_canonical_systems_doc() -> None:
    review_summary = _read(
        _repo_root() / "docs/reviews/2026-03-04-flux-makerv3-strategy-refactor-external-review-summary.md"
    )
    implementation_plan = _read(
        _repo_root() / "docs/plans/2026-03-04-flux-makerv3-strategy-refactor.md"
    )

    for content in (review_summary, implementation_plan):
        assert "systems/flux/docs/makerv3.md" in content
        assert "docs/flux/makerv3.md" not in content
