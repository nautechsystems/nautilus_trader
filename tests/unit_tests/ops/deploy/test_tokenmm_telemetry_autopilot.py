from __future__ import annotations

from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_tokenmm_telemetry_bootstrap_contract_is_documented() -> None:
    repo_root = _repo_root()
    bootstrap_script = _read(repo_root / "ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh")
    readme = _read(repo_root / "deploy/tokenmm/README.md")
    telemetry_runbook = _read(repo_root / "deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md")
    rds_env_example = _read(repo_root / "deploy/tokenmm/systemd/tokenmm-telemetry-rds.env.example")

    assert "--dry-run" in bootstrap_script
    assert "--apply-host-env" in bootstrap_script
    assert "aws rds describe-db-instances" in bootstrap_script
    assert "aws rds create-db-instance" in bootstrap_script
    assert "aws secretsmanager" in bootstrap_script
    assert "NAUTILUS_TELEMETRY_PG_SECRET_ID" in bootstrap_script
    assert "db-subnet-group" in bootstrap_script
    assert "vpc-security-group-ids" in bootstrap_script

    assert "NAUTILUS_TELEMETRY_PG_SECRET_ID=" in rds_env_example
    assert "NAUTILUS_TELEMETRY_PG_DATABASE=nautilus_telemetry" in rds_env_example
    assert "NAUTILUS_TELEMETRY_PG_SCHEMA=telemetry" in rds_env_example
    assert "TOKENMM_AWS_REGION=ap-southeast-1" in rds_env_example

    assert "bootstrap_tokenmm_telemetry_rds.sh" in readme
    assert "--apply-host-env" in readme
    assert "bootstrap_tokenmm_telemetry_rds.sh" in telemetry_runbook
    assert "--apply-host-env" in telemetry_runbook


def test_tokenmm_telemetry_runtime_contract_uses_wrapper_and_guardrails() -> None:
    repo_root = _repo_root()
    install_script = _read(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    wrapper_script = _read(repo_root / "ops/scripts/deploy/run_tokenmm_telemetry_shipper.sh")
    common_env = _read(repo_root / "deploy/tokenmm/systemd/common.env.example")
    health_service = _read(repo_root / "deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.service")
    health_timer = _read(repo_root / "deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.timer")
    healthcheck = _read(repo_root / "ops/scripts/deploy/tokenmm_telemetry_healthcheck.py")
    cutover = _read(repo_root / "ops/scripts/deploy/tokenmm_telemetry_cutover.py")

    assert "run_tokenmm_telemetry_shipper.sh" in install_script
    assert "tokenmm_telemetry_healthcheck.py" in install_script
    assert "flux-tokenmm-telemetry-health.timer" in install_script

    assert "NAUTILUS_TELEMETRY_PG_SECRET_ID" in common_env
    assert "TOKENMM_AWS_REGION=ap-southeast-1" in common_env

    assert "aws secretsmanager get-secret-value" in wrapper_script
    assert "NAUTILUS_TELEMETRY_PG_SECRET_ID" in wrapper_script
    assert "exit 78" in wrapper_script

    assert "ExecStart=" in health_service
    assert "tokenmm_telemetry_healthcheck.py" in health_service
    assert "OnCalendar=" in health_timer
    assert "Persistent=true" in health_timer

    assert "--max-telemetry-dir-gb" in healthcheck
    assert "--max-root-usage-pct" in healthcheck
    assert "--max-shipper-lag-minutes" in healthcheck
    assert "shipper_state.sqlite" in healthcheck

    assert "--dry-run" in cutover
    assert "--wait-for-catchup" in cutover
    assert "--delete-local-after-cutover" in cutover
    assert "tokenmm-telemetry-shipper" in cutover


def test_tokenmm_lean_autopilot_contract_is_documented() -> None:
    repo_root = _repo_root()
    design_doc = _read(repo_root / "docs/plans/2026-03-26-aws-managed-prod-boxes-design.md")
    implementation_plan = _read(repo_root / "docs/plans/2026-03-26-aws-managed-prod-boxes.md")

    assert "The supported operator workflow is:" in design_doc
    assert "manual AWS console clicking" in design_doc
    assert "CloudWatch Logs: `7 days` default" in design_doc
    assert "S3 raw archive: `7 days` by default" in design_doc
    assert "RDS should be added later, not now" in design_doc

    assert "durable history exported to `S3` and queryable through `Athena`" in implementation_plan
    assert "short default retention for logs and raw quote-cycle history" in implementation_plan
    assert "`RDS` to be optional and deferred" in implementation_plan


def test_tokenmm_cutover_contract_supports_archive_staging_and_athena() -> None:
    repo_root = _repo_root()
    cutover = _read(repo_root / "ops/scripts/deploy/tokenmm_telemetry_cutover.py")
    readme = _read(repo_root / "deploy/tokenmm/README.md")
    telemetry_runbook = _read(repo_root / "deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md")

    assert "--archive-staging-dir" in cutover
    assert "--archive-s3-bucket" in cutover
    assert "--archive-s3-prefix" in cutover
    assert "--athena-database" in cutover
    assert "--athena-workgroup" in cutover
    assert "--archive-quote-cycles" in cutover
    assert "archive_rotated_quote_cycle_db" in cutover
    assert "archive_rotated_sqlite_database" in cutover
    assert '"athena"' in cutover
    assert '"start-query-execution"' in cutover
    assert "athena_partition_sql" in cutover

    assert "lean default archive target is `S3 + Athena`" in readme
    assert "--archive-quote-cycles" in readme
    assert "Verify in Athena" in telemetry_runbook
    assert "S3 + Athena" in telemetry_runbook
