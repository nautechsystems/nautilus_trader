#!/usr/bin/env bash
# One-shot purge of orphaned .dev wheels in R2.
#
# These are dev wheels for platforms that the develop publish pipeline no longer
# builds (cp311 retired; Windows/macOS/manylinux_aarch64 moved to nightly-only).
# The retention script `publish-wheels-r2-remove-old-wheels.sh` keeps
# "most-recent per platform" but has no concept of "retired platform", so these
# orphans sit as group-of-one forever.
#
# Usage:
#   AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... CLOUDFLARE_R2_URL=https://... \
#     bash scripts/purge-orphan-dev-wheels.sh         # dry-run, lists actions only
#   AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... CLOUDFLARE_R2_URL=https://... \
#     bash scripts/purge-orphan-dev-wheels.sh --apply # actually delete + regen index

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

REPO_ROOT="${REPO_ROOT:-}"
if [[ -z "$REPO_ROOT" ]]; then
  candidate="$(cd "${SCRIPT_DIR}/.." && pwd)"
  if [[ -f "${candidate}/scripts/ci/publish-wheels-generate-index.sh" ]]; then
    REPO_ROOT="$candidate"
  fi
fi

BUCKET="${CLOUDFLARE_R2_BUCKET_NAME:-packages}"
PREFIX="${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}"
PREFIX="${PREFIX%/}"
APPLY=false
case "${1:-}" in
  "")
    ;;
  "--apply")
    APPLY=true
    ;;
  *)
    echo "ERROR: unknown argument: ${1}" >&2
    echo "Usage: bash $0 [--apply]" >&2
    exit 1
    ;;
esac

# Run aws without inherited PYTHONHOME from the user's shell.
AWS=(env -u PYTHONHOME aws)

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" || -z "${CLOUDFLARE_R2_URL:-}" ]]; then
  echo "ERROR: set AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, CLOUDFLARE_R2_URL" >&2
  exit 1
fi
export AWS_DEFAULT_REGION="${AWS_DEFAULT_REGION:-${CLOUDFLARE_R2_REGION:-auto}}"

if ! command -v aws > /dev/null 2>&1; then
  echo "ERROR: aws CLI not found on PATH" >&2
  echo "Cloudflare R2 is S3-compatible, so this script uses aws with --endpoint-url." >&2
  echo "Install AWS CLI v2, then re-run this script." >&2
  exit 1
fi

if [[ "$APPLY" == true ]]; then
  if [[ -z "$REPO_ROOT" || ! -d "$REPO_ROOT" ]]; then
    echo "ERROR: could not find repo root. Set REPO_ROOT=/path/to/nautilus_trader" >&2
    exit 1
  fi

  for script in \
    "scripts/ci/publish-wheels-generate-index.sh" \
    "scripts/ci/publish-wheels-r2-upload-index.sh"; do
    if [[ ! -f "${REPO_ROOT}/${script}" ]]; then
      echo "ERROR: missing ${REPO_ROOT}/${script}" >&2
      exit 1
    fi
  done

  if compgen -G "${REPO_ROOT}/dist/nautilus_trader-*.whl" > /dev/null; then
    echo "ERROR: local dist wheels exist under ${REPO_ROOT}/dist" >&2
    echo "Move them before regenerating the R2 index, or they can enter index.html without upload." >&2
    exit 1
  fi
fi

ORPHANS=(
  "nautilus_trader-1.221.0.dev20251026+11610-cp311-cp311-macosx_15_0_arm64.whl"
  "nautilus_trader-1.221.0.dev20251026+11610-cp311-cp311-manylinux_2_35_x86_64.whl"
  "nautilus_trader-1.221.0.dev20251026+11610-cp311-cp311-win_amd64.whl"
  "nautilus_trader-1.226.0.dev20260418+14628-cp312-cp312-win_amd64.whl"
  "nautilus_trader-1.226.0.dev20260418+14628-cp313-cp313-win_amd64.whl"
  "nautilus_trader-1.226.0.dev20260418+14628-cp314-cp314-win_amd64.whl"
  "nautilus_trader-1.226.0.dev20260428+14797-cp312-cp312-macosx_15_0_arm64.whl"
  "nautilus_trader-1.226.0.dev20260428+14797-cp313-cp313-macosx_15_0_arm64.whl"
  "nautilus_trader-1.226.0.dev20260428+14797-cp314-cp314-macosx_15_0_arm64.whl"
  "nautilus_trader-1.226.0.dev20260428+14798-cp312-cp312-manylinux_2_35_aarch64.whl"
  "nautilus_trader-1.226.0.dev20260428+14798-cp313-cp313-manylinux_2_35_aarch64.whl"
  "nautilus_trader-1.226.0.dev20260428+14798-cp314-cp314-manylinux_2_35_aarch64.whl"
)

echo "Mode: $([[ "$APPLY" == true ]] && echo APPLY || echo DRY-RUN)"
echo "Bucket: s3://${BUCKET}/${PREFIX}/"
echo "Endpoint: ${CLOUDFLARE_R2_URL}"
echo ""
echo "Reading bucket listing..."
bucket_uri="s3://${BUCKET}/${PREFIX}/"
if ! bucket_listing="$("${AWS[@]}" s3 ls "$bucket_uri" --endpoint-url="${CLOUDFLARE_R2_URL}" 2>&1)"; then
  echo "ERROR: failed to list ${bucket_uri}" >&2
  echo "$bucket_listing" >&2
  exit 1
fi

declare -A existing_names=()
while IFS= read -r name; do
  [[ -n "$name" ]] && existing_names["$name"]=1
done < <(printf '%s\n' "$bucket_listing" | awk '{print $4}')

echo "Confirming presence of orphans before delete..."
present_count=0
missing_count=0
for f in "${ORPHANS[@]}"; do
  if [[ -n "${existing_names[$f]:-}" ]]; then
    echo "  present: ${f}"
    present_count=$((present_count + 1))
  else
    echo "  MISSING (already deleted?): ${f}"
    missing_count=$((missing_count + 1))
  fi
done
echo "Summary: ${present_count} present, ${missing_count} missing"
echo ""

if [[ "$APPLY" != true ]]; then
  echo "Dry-run complete. Re-run with --apply to delete and regenerate the index."
  exit 0
fi

echo "Deleting ${present_count} present orphan wheels..."
for f in "${ORPHANS[@]}"; do
  if [[ -n "${existing_names[$f]:-}" ]]; then
    echo "  rm s3://${BUCKET}/${PREFIX}/${f}"
    "${AWS[@]}" s3 rm "s3://${BUCKET}/${PREFIX}/${f}" --endpoint-url="${CLOUDFLARE_R2_URL}"
  else
    echo "  skip missing: ${f}"
  fi
done

echo ""
echo "Regenerating index.html via existing CI scripts..."
cd "${REPO_ROOT}"
export CLOUDFLARE_R2_BUCKET_NAME="${BUCKET}"
export CLOUDFLARE_R2_PREFIX="${PREFIX}"
export CLOUDFLARE_R2_REGION="${CLOUDFLARE_R2_REGION:-auto}"

# Re-export AWS_* and CLOUDFLARE_R2_URL for the sub-scripts.
env -u PYTHONHOME bash ./scripts/ci/publish-wheels-generate-index.sh
env -u PYTHONHOME bash ./scripts/ci/publish-wheels-r2-upload-index.sh

echo ""
echo "Verifying final state..."
if ! final_listing="$("${AWS[@]}" s3 ls "s3://${BUCKET}/${PREFIX}/" --endpoint-url="${CLOUDFLARE_R2_URL}" 2>&1)"; then
  echo "ERROR: failed to list final state" >&2
  echo "$final_listing" >&2
  exit 1
fi
if grep "\.dev" <<< "$final_listing"; then
  :
else
  echo "  (no .dev wheels remaining matched)"
fi
echo ""
echo "Done."
