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

# Clean up alpha (.a) wheels on the nightly branch
if [[ "$branch_name" == "nightly" ]]; then
  echo "Cleaning up alpha wheels for the nightly branch..."
  echo "All files before filtering:"
  echo "$files"

  # First find unique platform suffixes
  platform_tags=$(echo "$files" | grep -E "a[0-9]{8}" | sed -E 's/.*-(cp[^.]+).whl$/\1/' | sort -u)
  echo "Found platform tags:"
  echo "$platform_tags"

  had_failures=false
  for platform_tag in $platform_tags; do
    echo "Processing platform: $platform_tag"

    # Get all alpha wheels for this platform
    matching_files=$(echo "$files" | grep -E "a[0-9]{8}.*-${platform_tag}\.whl$" | sort -t'a' -k2 -V)

    echo "Matching files:"
    echo "$matching_files"

    # Extract unique versions (dates) from matching files
    versions=$(echo "$matching_files" | sed -E "s/^.+-[0-9]+\.[0-9]+\.[0-9]+a([0-9]{8})-.+\.whl$/\1/" | sort -n)
    echo "Unique versions (dates) for platform: $versions"

    # Retain only the wheels in the lookback
    versions_to_keep=$(echo "$versions" | tail -n $NIGHTLY_LOOKBACK)
    echo "Versions to keep: $versions_to_keep"

    # Delete files outside lookback
    for file in $matching_files; do
      file_version=$(echo "$file" | sed -E "s/^.+-[0-9]+\.[0-9]+\.[0-9]+a([0-9]{8})-.+\.whl$/\1/")
      if echo "$versions_to_keep" | grep -qx "$file_version"; then
        echo "Keeping wheel: $file"
      else
        echo "Deleting old .a wheel: $file"
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
  echo "Finished cleaning up .a wheels"
  if [ "$had_failures" = true ]; then
    echo "Prune completed with failures on nightly branch"
    exit 1
  fi
fi
