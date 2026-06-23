#!/usr/bin/env bash
set -euo pipefail

# Number of wheel versions to retain on nightly branch
NIGHTLY_LOOKBACK=30

echo "Cleaning up old wheels in Cloudflare R2..."

branch_name="${GITHUB_REF_NAME}" # Get the current branch
files=$(aws s3 ls "s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/" \
  --endpoint-url="${CLOUDFLARE_R2_URL}" --cli-connect-timeout 10 --cli-read-timeout 60 | awk '{print $4}')
if [ -z "$files" ]; then
  echo "No files found for cleanup"
  exit 0
fi

echo "Current wheels:"
echo "$files"
echo "---"

# Skip index.html
files=$(echo "$files" | grep -v "^index\.html$")

# Clean up dev wheels on the develop branch
if [[ "$branch_name" == "develop" ]]; then
  echo "Cleaning up .dev wheels for the develop branch..."
  echo "All files before filtering:"
  echo "$files"

  # First find unique platform suffixes
  platform_tags=$(echo "$files" | grep "\.dev" | sed -E 's/.*-(cp[^.]+).whl$/\1/' | sort -u)
  echo "Found platform tags:"
  echo "$platform_tags"

  had_failures=false
  for platform_tag in $platform_tags; do
    echo "Processing platform: $platform_tag"

    # Get all dev wheels for this platform
    matching_files=$(echo "$files" | grep "\.dev.*-${platform_tag}\.whl$" | sort -t'+' -k2 -V)

    echo "Matching files:"
    echo "$matching_files"

    # Keep only the latest version
    latest=$(echo "$matching_files" | tail -n 1)
    echo "Latest version to keep: $latest"

    # Delete all but the latest
    for file in $matching_files; do
      if [[ "$file" != "$latest" ]]; then
        echo "Deleting old .dev wheel: $file"
        success=false
        for i in {1..5}; do
          if aws s3 rm "s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/$file" \
            --endpoint-url="${CLOUDFLARE_R2_URL}" --cli-connect-timeout 10 --cli-read-timeout 60; then
            success=true
            break
          else
            echo "Delete failed for $file, retrying ($i/5)..."
            sleep $((2 ** i))
          fi
        done
        if [ "$success" = false ]; then
          echo "Warning: Failed to delete $file after retries, skipping..."
          had_failures=true
        fi
      else
        echo "Keeping wheel: $file"
      fi
    done
  done
  echo "Finished cleaning up .dev wheels"
  if [ "$had_failures" = true ]; then
    echo "Prune completed with failures on develop branch"
    exit 1
  fi
fi

# Clean up nightly wheels on the nightly branch. Matches the legacy `aYYYYMMDD` form and, when
# the base is a pre-release (e.g. 2.0.0rc1), the `.devYYYYMMDD` form. Develop's
# `.devYYYYMMDD+run` wheels live in the same index but are excluded because the date marker is
# matched only when it directly precedes the platform tag (no `+run` local segment).
if [[ "$branch_name" == "nightly" ]]; then
  echo "Cleaning up nightly wheels for the nightly branch..."
  echo "All files before filtering:"
  echo "$files"

  # First find unique platform suffixes
  platform_tags=$(echo "$files" | grep -E "(a[0-9]{8}|\.dev[0-9]{8})-" | sed -E 's/.*-(cp[^.]+).whl$/\1/' | sort -u)
  echo "Found platform tags:"
  echo "$platform_tags"

  had_failures=false
  for platform_tag in $platform_tags; do
    echo "Processing platform: $platform_tag"

    # Get all nightly wheels for this platform; the date marker must directly precede the
    # platform tag, which excludes develop's `.dev...+run` wheels
    matching_files=$(echo "$files" | grep -E "(a[0-9]{8}|\.dev[0-9]{8})-.*-${platform_tag}\.whl$" | sort -V)

    echo "Matching files:"
    echo "$matching_files"

    # Extract unique versions (dates) from matching files
    # Dedupe by date so the lookback counts distinct dates (a transition day may carry both an
    # alpha and a .dev wheel for the same date).
    versions=$(echo "$matching_files" | sed -E "s/.*(a|\.dev)([0-9]{8})-.+\.whl$/\2/" | sort -n -u)
    echo "Unique versions (dates) for platform: $versions"

    # Retain only the wheels in the lookback
    versions_to_keep=$(echo "$versions" | tail -n $NIGHTLY_LOOKBACK)
    echo "Versions to keep: $versions_to_keep"

    # Delete files outside lookback
    for file in $matching_files; do
      file_version=$(echo "$file" | sed -E "s/.*(a|\.dev)([0-9]{8})-.+\.whl$/\2/")
      if echo "$versions_to_keep" | grep -qx "$file_version"; then
        echo "Keeping wheel: $file"
      else
        echo "Deleting old nightly wheel: $file"
        success=false
        for i in {1..5}; do
          if aws s3 rm "s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/$file" \
            --endpoint-url="${CLOUDFLARE_R2_URL}" --cli-connect-timeout 10 --cli-read-timeout 60; then
            success=true
            break
          else
            echo "Delete failed for $file, retrying ($i/5)..."
            sleep $((2 ** i))
          fi
        done
        if [ "$success" = false ]; then
          echo "Warning: Failed to delete $file after retries, skipping..."
          had_failures=true
        fi
      fi
    done
  done
  echo "Finished cleaning up nightly wheels"
  if [ "$had_failures" = true ]; then
    echo "Prune completed with failures on nightly branch"
    exit 1
  fi
fi
