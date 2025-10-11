#!/usr/bin/env bash
#
# curate-dataset.sh  ── Curate and package an external dataset for test fixtures.
#
# This script downloads the specified dataset file, captures its licence
# information, computes a SHA-256 checksum, and emits a ready-to-upload
# directory structure that is compatible with the NautilusTrader test-data
# bucket layout described in the developer guide.
#
# Usage:
#   scripts/curate-dataset.sh <slug> <filename> <download-url> <licence>
#
# Example:
#   scripts/curate-dataset.sh fi2010_day1 lob_5stocks.csv.gz \
#       "https://example.com/fi2010/lob_5stocks.csv.gz" CC-BY-SA-4.0
#
# The command above will create the following local directory structure:
#   v1/fi2010_day1/
#     ├── lob_5stocks.csv.gz    # downloaded file
#     ├── LICENSE.txt           # contains the licence string or URL
#     └── metadata.json         # provenance & integrity metadata
#
# The resulting directory can be uploaded to the cloud bucket one-to-one
# (e.g. using `aws s3 cp --recursive`).
#
# Notes:
# • Only basic validation is performed. The caller is responsible for
#   ensuring that the licence really permits redistribution.
# • `curl` and `sha256sum` must be available in the environment.

set -euo pipefail

if [[ $# -lt 4 ]]; then
  echo "Usage: $0 <slug> <filename> <download-url> <licence>" >&2
  exit 1
fi

# Positional arguments
slug="$1"    # Dataset directory name, e.g. fi2010_day1
file="$2"    # Output filename, e.g. lob_5stocks.csv.gz
url="$3"     # Original download URL
licence="$4" # Licence identifier or URL (e.g. CC-BY-SA-4.0)

# Create target directory under versioned root
root_dir="v1/${slug}"
mkdir -p "${root_dir}"

target_path="${root_dir}/${file}"

# Download the data file (overwrite if exists)
echo "[curate-dataset] Downloading dataset from ${url}" >&2
curl -L --fail --retry 3 -o "${target_path}" "${url}"

# Compute checksum and size
sha256=$(sha256sum "${target_path}" | awk '{print $1}')
size_bytes=$(stat -c%s "${target_path}")

# Write LICENCE file (overwrites if already present)
echo "${licence}" > "${root_dir}/LICENSE.txt"

# Write metadata.json
timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

cat > "${root_dir}/metadata.json" << JSON
{
  "file": "${file}",
  "sha256": "${sha256}",
  "size_bytes": ${size_bytes},
  "original_url": "${url}",
  "licence": "${licence}",
  "added_at": "${timestamp}"
}
JSON

echo "[curate-dataset] Dataset packaged under ${root_dir}" >&2
