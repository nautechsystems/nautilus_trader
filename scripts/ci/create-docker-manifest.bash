#!/usr/bin/env bash
set -euo pipefail

image=${1:?Usage: create-docker-manifest.bash IMAGE}
metadata=${DOCKER_METADATA_OUTPUT_JSON:?DOCKER_METADATA_OUTPUT_JSON is required}
output=${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}

if [[ ! "$image" =~ ^ghcr\.io/[0-9A-Za-z_.-]+/[0-9A-Za-z_.-]+$ ]]; then
  echo "::error::Invalid container image: $image" >&2
  exit 1
fi
if ! jq -e '.tags | type == "array" and length > 0 and all(.[]; type == "string")' \
  > /dev/null <<< "$metadata"; then
  echo "::error::Docker metadata did not contain a valid tag list" >&2
  exit 1
fi

tags=()
while IFS= read -r tag; do
  tag_value=${tag#"$image:"}
  if [[ "$tag" != "$image:"* ]] ||
    [[ ! "$tag_value" =~ ^[0-9A-Za-z_][0-9A-Za-z_.-]{0,127}$ ]]; then
    echo "::error::Invalid Docker metadata tag for $image: $tag" >&2
    exit 1
  fi
  tags+=("-t" "$tag")
done < <(jq -r '.tags[]' <<< "$metadata")

shopt -s nullglob
digest_files=(*)
shopt -u nullglob
if ((${#digest_files[@]} == 0)); then
  echo "::error::No image digest files found" >&2
  exit 1
fi

digests=()
for digest_file in "${digest_files[@]}"; do
  if [ ! -f "$digest_file" ] || [[ ! "$digest_file" =~ ^[0-9a-f]{64}$ ]]; then
    echo "::error::Invalid image digest file: $digest_file" >&2
    exit 1
  fi
  digests+=("$image@sha256:$digest_file")
done

docker buildx imagetools create "${tags[@]}" "${digests[@]}"
tag=$(jq -r '.tags[0]' <<< "$metadata")
digest=$(docker buildx imagetools inspect "$tag" --format '{{json .Manifest}}' | jq -r '.digest')
if [[ ! "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "::error::Invalid image digest: $digest" >&2
  exit 1
fi

printf 'digest=%s\ndigest_hex=%s\n' "$digest" "${digest#sha256:}" >> "$output"
