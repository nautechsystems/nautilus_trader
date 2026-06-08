#!/usr/bin/env bash
# Generate PyPI publish attestations with bounded retry.
set -euo pipefail

attempts="${PYPI_ATTESTATION_SIGN_ATTEMPTS:-5}"
retry_delay_seconds="${PYPI_ATTESTATION_SIGN_RETRY_DELAY_SECONDS:-15}"
pypi_attestations_version="${PYPI_ATTESTATIONS_VERSION:-$(bash scripts/tool-version.sh pypi-attestations)}"

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    exit 1
  fi
}

validate_positive_integer PYPI_ATTESTATION_SIGN_ATTEMPTS "$attempts"
validate_positive_integer PYPI_ATTESTATION_SIGN_RETRY_DELAY_SECONDS "$retry_delay_seconds"

if [[ "$#" -eq 0 ]]; then
  echo "::error::Usage: sign-pypi-attestations.bash <artifact> [<artifact>...]"
  exit 1
fi
if ! command -v uv > /dev/null; then
  echo "::error::uv not found."
  exit 1
fi

artifacts=()
for artifact in "$@"; do
  if [[ ! -f "$artifact" ]]; then
    echo "::error::PyPI artifact not found: $artifact"
    exit 1
  fi
  artifacts+=("$artifact")
done

status=0
delay="$retry_delay_seconds"

for attempt in $(seq 1 "$attempts"); do
  for artifact in "${artifacts[@]}"; do
    rm -f "${artifact}.publish.attestation"
  done

  set +e
  uv run --no-project --no-build --with "pypi-attestations==${pypi_attestations_version}" -- \
    python -m pypi_attestations sign "${artifacts[@]}"
  status=$?
  set -e

  if [[ "$status" -eq 0 ]]; then
    echo "Generated PyPI publish attestations for ${#artifacts[@]} artifact(s)."
    exit 0
  fi

  if [[ "$attempt" -lt "$attempts" ]]; then
    echo "pypi-attestations sign failed (exit=${status}), retry (${attempt}/${attempts}) after ${delay}s"
    sleep "$delay"
    delay=$((delay * 2))
  fi
done

echo "::error::pypi-attestations sign failed after ${attempts} attempts."
exit "$status"
