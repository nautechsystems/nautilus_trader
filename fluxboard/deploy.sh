#!/usr/bin/env bash
set -euo pipefail

# Atomic deployment script for fluxboard
# Ensures clean deployment with no stale cached files

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$SCRIPT_DIR/dist"
STATIC_DIR="$(dirname "$SCRIPT_DIR")/static/fluxboard"
PROVENANCE_FILE="$STATIC_DIR/provenance.json"

UI_PROFILE="${FLUXBOARD_UI_PROFILE:-${VITE_FLUXBOARD_UI_PROFILE:-trader}}"
UPSTREAM_REPO="${FLUXBOARD_UPSTREAM_REPO:-}"
UPSTREAM_SHA="${FLUXBOARD_UPSTREAM_SHA:-}"
BUILT_AT_UTC="${FLUXBOARD_BUILT_AT_UTC:-$(date -u +"%Y-%m-%dT%H:%M:%SZ")}"

if [ -z "$UPSTREAM_REPO" ]; then
  UPSTREAM_REPO="$(git -C "$REPO_ROOT" config --get remote.origin.url 2> /dev/null || true)"
fi
if [ -z "$UPSTREAM_REPO" ]; then
  UPSTREAM_REPO="$(git -C "$REPO_ROOT" rev-parse --show-toplevel 2> /dev/null || echo "$REPO_ROOT")"
fi

if [ -z "$UPSTREAM_SHA" ]; then
  UPSTREAM_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD 2> /dev/null || true)"
fi

if ! [[ "$UPSTREAM_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  if [ "${FLUXBOARD_ALLOW_UNKNOWN_SHA:-0}" = "1" ]; then
    echo "⚠️  Fluxboard provenance: non-SHA upstream value allowed by FLUXBOARD_ALLOW_UNKNOWN_SHA=1"
  else
    echo "❌ Error: unable to resolve a full 40-char upstream SHA for provenance."
    echo "    Set FLUXBOARD_UPSTREAM_SHA=<40-char sha> or run from a git checkout."
    echo "    For local debugging only, override with FLUXBOARD_ALLOW_UNKNOWN_SHA=1."
    exit 1
  fi
fi

echo "🚀 Fluxboard Deployment"
echo "======================="

# Check if dist directory exists
if [ ! -d "$DIST_DIR" ]; then
  echo "❌ Error: dist/ directory not found. Run 'pnpm run build' first."
  exit 1
fi

# Backup current deployment (in case of rollback)
# Disabled by default; set FLUXBOARD_BACKUP=1 to enable
if [ -d "$STATIC_DIR" ] && [ "${FLUXBOARD_BACKUP:-0}" = "1" ]; then
  BACKUP_DIR="${STATIC_DIR}.backup.$(date +%s)"
  echo "📦 Backing up current deployment to: $BACKUP_DIR"
  cp -r "$STATIC_DIR" "$BACKUP_DIR"

  # Keep only last 3 backups
  # shellcheck disable=SC2012 # Backup dirs are created by this script with sanitized names.
  ls -dt "${STATIC_DIR}.backup."* 2> /dev/null | tail -n +4 | xargs rm -rf 2> /dev/null || true
elif [ -d "$STATIC_DIR" ]; then
  echo "ℹ️  Skipping backup (set FLUXBOARD_BACKUP=1 to enable)"
fi

# Ensure static directory exists (do not delete to preserve old hashed assets)
mkdir -p "$STATIC_DIR"
mkdir -p "$STATIC_DIR/assets"

# Copy new build without removing previous hashed assets
echo "📁 Copying new build (preserving old assets)..."
cp -r "$DIST_DIR"/assets/* "$STATIC_DIR/assets/"
cp -f "$DIST_DIR/index.html" "$STATIC_DIR/index.html"
cp -f "$DIST_DIR/favicon.svg" "$STATIC_DIR/favicon.svg"

# Emit bundle provenance for downstream artifact consumers.
python3 - "$PROVENANCE_FILE" "$UPSTREAM_REPO" "$UPSTREAM_SHA" "$UI_PROFILE" "$BUILT_AT_UTC" << 'PY'
import json
import pathlib
import sys

target, upstream_repo, upstream_sha, ui_profile, built_at_utc = sys.argv[1:6]
path = pathlib.Path(target)
payload = {
    "upstream_repo": upstream_repo,
    "upstream_sha": upstream_sha,
    "ui_profile": ui_profile,
    "built_at_utc": built_at_utc,
}
path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

# Verify deployment
EXPECTED_FILES=("index.html" "favicon.svg" "assets" "provenance.json")
for file in "${EXPECTED_FILES[@]}"; do
  if [ ! -e "$STATIC_DIR/$file" ]; then
    echo "❌ Error: Missing $file in deployment"
    exit 1
  fi
done

# Count assets
ASSET_COUNT=$(find "$STATIC_DIR/assets" -mindepth 1 -maxdepth 1 -type f -print 2> /dev/null | wc -l | xargs)
echo "✅ Deployed $ASSET_COUNT asset files"

# Show what was deployed
echo ""
echo "📋 Deployed Files:"
ls -lh "$STATIC_DIR"
echo ""
echo "📋 Assets:"
ls -lh "$STATIC_DIR/assets"
echo ""
echo "📋 Provenance:"
cat "$PROVENANCE_FILE"

# Extract asset hashes from index.html for verification
echo ""
echo "🔍 Asset Hashes:"
grep -oP 'assets/index-\K[^.]+' "$STATIC_DIR/index.html" | head -2

echo ""
echo "✅ Deployment complete!"
echo "🌐 Access at: http://localhost:5000/"
if [ -n "${BACKUP_DIR:-}" ]; then
  echo ""
  echo "💡 To rollback: mv ${BACKUP_DIR} ${STATIC_DIR}"
fi

# Optional: prune very old assets to avoid unbounded growth (kept disabled by default)
# find "$STATIC_DIR/assets" -type f -mtime +30 -print -delete
