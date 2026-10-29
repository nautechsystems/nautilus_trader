#!/usr/bin/env bash
set -euo pipefail

main() {
  if [[ $# -ne 4 || ("$3" != "recover" && "$3" != "verify" && "$3" != "prepare-pypi") ]]; then
    echo "Usage: $0 <tag> <source-run-id> <recover|verify|prepare-pypi> <diagnostics-dir>" >&2
    exit 1
  fi
  export TAG_NAME=$1
  source_run=$2
  mode=$3
  mkdir -p "$4"
  diagnostics="$(cd -- "$4" && pwd)"
  work_dir="$(mktemp -d "${diagnostics}/recovery.XXXXXX")"
  exec > >(tee "${work_dir}/recovery.log") 2>&1
  script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
  export RELEASE_VERIFICATION_DIAGNOSTICS_DIR="${work_dir}/verification"
  mkdir -p "$RELEASE_VERIFICATION_DIAGNOSTICS_DIR"
  trap 'printf "%s\n" "$?" > "${work_dir}/exit-status"' EXIT
  run_step source validate_source
  source_sha="$(jq -r '.head_sha' "${work_dir}/source/source-run.json")"
  source_attempt="$(jq -r '.run_attempt' "${work_dir}/source/source-run.json")"
  if [[ "$(git rev-parse HEAD)" != "$source_sha" ]]; then
    echo "Error: Run recovery from a checkout of the validated release commit" >&2
    exit 1
  fi
  export RELEASE_RECOVERY_SHA="${GITHUB_SHA:-}"
  export GITHUB_SHA="$source_sha"
  export PACKAGE_VERSION="${TAG_NAME#v}"
  source_version="$(python3 -c 'import tomllib; print(tomllib.load(open("python/pyproject.toml", "rb"))["project"]["version"])')"
  if [[ "$source_version" != "$PACKAGE_VERSION" ]]; then
    echo "Error: Source package version does not match the release tag" >&2
    exit 1
  fi
  export ATTESTATION_IDENTITY="https://github.com/${repository}/.github/workflows/build.yml@refs/heads/master"
  export ATTESTATION_ISSUER=https://token.actions.githubusercontent.com
  for tool in gh uv cargo jq; do
    "$tool" --version >> "${work_dir}/tool-versions.txt" 2>&1
  done
  gh attestation verify --help > "${work_dir}/attestation-options.txt"
  gh release verify --help > "${work_dir}/release-options.txt"
  grep -Eq '^[[:space:]]+gh release verify([[:space:]]|$)' "${work_dir}/release-options.txt" || {
    echo "Error: GitHub CLI must support gh release verify" >&2
    exit 1
  }
  grep -Fq -- '--source-digest' "${work_dir}/attestation-options.txt" || {
    echo "Error: GitHub CLI must support attestation verification with --source-digest" >&2
    exit 1
  }
  assets="${work_dir}/assets"
  remote="${work_dir}/remote"
  mkdir -p "$assets" "$remote"
  gh release view "$TAG_NAME" --repo "$repository" --json tagName,isDraft,assets \
    > "${work_dir}/release.json"
  jq -e --arg tag "$TAG_NAME" '
    .tagName == $tag and (.isDraft | type == "boolean")
    and ([.assets[].name] | length == (unique | length))
    and all(.assets[]; (.name | test("^[A-Za-z0-9_.+-]+$"))
      and (.state // "uploaded") == "uploaded" and .size > 0)
  ' "${work_dir}/release.json" > /dev/null || {
    echo "Error: Release metadata has unexpected identity, duplicate names, or incomplete assets" >&2
    jq -r '.assets[] | select((.state // "uploaded") != "uploaded" or .size <= 0)
      | "Incomplete release asset: \(.name)"' "${work_dir}/release.json" >&2
    exit 1
  }
  if jq -e '.isDraft == false' "${work_dir}/release.json" > /dev/null; then
    if [[ "$mode" == "prepare-pypi" ]]; then
      echo "Error: Publication recovery requires a draft release" >&2
      exit 1
    fi
    mode=verify
  elif [[ "$mode" == "verify" ]]; then
    echo "Error: Verification mode requires an already published release" >&2
    exit 1
  fi
  if [[ "$(jq '.assets | length' "${work_dir}/release.json")" -gt 0 ]]; then
    run_step download-release gh release download "$TAG_NAME" --repo "$repository" --dir "$remote"
  fi
  if [[ "$mode" != "verify" ]]; then
    restore_source_artifacts
  fi
  for asset in "$remote"/*; do
    [[ -f "$asset" ]] || continue
    name="${asset##*/}"
    if [[ -f "${assets}/${name}" ]]; then
      cmp -s "$asset" "${assets}/${name}" || {
        echo "Error: Existing release asset content mismatch: ${name}" >&2
        exit 1
      }
    else
      cp "$asset" "${assets}/${name}"
    fi
  done
  validate_packages
  if [[ "$mode" == "prepare-pypi" ]]; then
    run_step prepare-pypi python3 "${script_dir}/release-recovery.py" prepare "$work_dir"
    recheck_source
    return
  fi
  if [[ "$mode" == "recover" && -n "${PYPI_RECOVERY_RUN_ID:-}" ]]; then
    run_step publication-evidence python3 "${script_dir}/release-recovery.py" \
      download "$work_dir" "$PYPI_RECOVERY_RUN_ID"
  fi
  run_step generate-checksums bash "${script_dir}/publish-release-checksums.sh" --generate-local "$assets"
  preserve_manifest dist-manifest.json
  for package in "$assets"/*.whl "$assets"/*.tar.gz; do
    for suffix in sigstore intoto.jsonl; do
      sibling="${package}.${suffix}"
      [[ -s "$sibling" ]] || {
        echo "Error: Missing source attestation sibling: ${sibling##*/}" >&2
        exit 1
      }
    done
    cp "${package}.sigstore" "${work_dir}/bundle.json"
    if ! run_step "bundle-${package##*/}" gh attestation verify "$package" --bundle "${work_dir}/bundle.json" \
      --repo "$repository" --source-digest "$source_sha" --source-ref refs/heads/master \
      --cert-identity "$ATTESTATION_IDENTITY" --cert-oidc-issuer "$ATTESTATION_ISSUER"; then
      run_step "recovery-bundle-${package##*/}" python3 "${script_dir}/release-recovery.py" \
        verify "$package" "${work_dir}/source/source-run.json"
      ATTESTATION_PREDICATE_TYPE="$(jq -r '.dsseEnvelope.payload | @base64d | fromjson | .predicateType' \
        "${package}.sigstore")"
      export ATTESTATION_PREDICATE_TYPE
    fi
    jq -eSc '.dsseEnvelope // empty' "${package}.sigstore" > "${work_dir}/expected-envelope.json"
    jq -eSc '.' "${package}.intoto.jsonl" > "${work_dir}/existing-envelope.json"
    cmp -s "${work_dir}/expected-envelope.json" "${work_dir}/existing-envelope.json" || {
      echo "Error: Attestation envelope mismatch: ${package##*/}" >&2
      exit 1
    }
  done
  run_step registries bash "${script_dir}/verify-published-registries.bash" "$assets"
  preserve_manifest crates-manifest.json
  assets_missing=()
  for asset in "$assets"/*; do
    [[ -f "$asset" ]] || continue
    name="${asset##*/}"
    if [[ -f "${remote}/${name}" ]]; then
      cmp -s "$asset" "${remote}/${name}" || {
        echo "Error: Existing release asset content mismatch: ${name}" >&2
        exit 1
      }
    elif [[ "$mode" == "verify" ]]; then
      echo "Error: Published release is missing ${name}" >&2
      exit 1
    else
      assets_missing+=("$asset")
    fi
  done
  if [[ "$mode" == "recover" ]]; then
    recheck_source
    if [[ "${#assets_missing[@]}" -gt 0 ]]; then
      run_step reconcile-assets env GH_RELEASE_UPLOAD_ATTEMPTS=1 \
        bash "${script_dir}/upload-github-release-assets.bash" \
        --verify-existing recovery "${assets_missing[@]}"
    fi
    run_step reconcile-notes bash "${script_dir}/publish-release-checksums.sh" --update-notes "$assets"
    run_step final-assets env GH_RELEASE_EXPECT_DRAFT=true \
      bash "${script_dir}/verify-github-release-assets.bash"
    run_step rekor bash "${script_dir}/check-rekor-ready.bash"
    recheck_source
    run_step publish bash "${script_dir}/publish-github-release.bash"
  else
    run_step final-assets env GH_RELEASE_EXPECT_DRAFT=false \
      bash "${script_dir}/verify-github-release-assets.bash"
  fi
  run_step sealed-release bash "${script_dir}/verify-github-release-attestation.bash"
  echo "Release ${TAG_NAME} ${mode} completed"
}

run_step() {
  local name=$1
  shift
  printf '%s\n' "$name" > "${work_dir}/checkpoint"
  "$@" 2>&1 | tee "${work_dir}/${name}.log"
}

validate_source() {
  local diagnostics="${work_dir}/source"
  if ! [[ "$TAG_NAME" == v* && "$source_run" =~ ^[1-9][0-9]*$ &&
    "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
    echo "Error: Invalid release tag, source run, or repository" >&2
    exit 1
  fi
  bash "${script_dir}/release-version-policy.bash" "${TAG_NAME#v}" > /dev/null
  mkdir -p "$diagnostics"
  gh api "repos/${repository}/actions/runs/${source_run}" > "${diagnostics}/source-run.json"
  gh api "repos/${repository}/commits/${TAG_NAME}" > "${diagnostics}/tag.json"
  gh api "repos/${repository}/actions/runs/${source_run}/jobs?filter=all&per_page=100" \
    --paginate --jq '.jobs[]' | jq -s '.' > "${diagnostics}/source-jobs.json"

  jq -e --arg repository "$repository" '
    .repository.full_name == $repository and .head_repository.full_name == $repository
    and .event == "push" and .head_branch == "master"
    and .path == ".github/workflows/build.yml" and .status == "completed"
    and (.head_sha | test("^[a-f0-9]{40}$"))
    and (.run_attempt | type == "number") and .run_attempt >= 1
  ' "${diagnostics}/source-run.json" > /dev/null || {
    echo "Error: Recovery requires a completed master push run of build.yml in this repository" >&2
    exit 1
  }
  local source_sha
  source_sha="$(jq -r '.head_sha' "${diagnostics}/source-run.json")"
  if [[ "$(jq -r '.sha' "${diagnostics}/tag.json")" != "$source_sha" ]]; then
    echo "Error: Release tag and source run identify different commits" >&2
    exit 1
  fi
  jq -e 'group_by(.name) | map(max_by(.run_attempt // 0))
    | any(.[]; .name == "tag-release" and .conclusion == "success")' \
    "${diagnostics}/source-jobs.json" > /dev/null || {
    echo "Error: Source run did not pass its tag-release prerequisite gates" >&2
    exit 1
  }
  echo "Validated ${TAG_NAME} against source run ${source_run} at ${source_sha}"
}

restore_source_artifacts() {
  local metadata="${work_dir}/source-artifacts.json"
  local downloaded="${work_dir}/source-artifacts"
  gh api "repos/${repository}/actions/runs/${source_run}/artifacts?per_page=100" \
    --paginate --jq '.artifacts[]' | jq -s '.' > "$metadata"
  jq --arg sha "$source_sha" --argjson run "$source_run" '
    [.[] | select(.expired == false)
      | select(.name | test("^nautilus_trader-.*[.]whl$|^release-sdist$|^release-.*-attestation-"))]
    | if all(.[]; .workflow_run.id == $run and .workflow_run.head_sha == $sha
        and (.name | test("^[A-Za-z0-9_.+-]+$")))
      and ([.[].name] | length == (unique | length)) then .
      else error("Source artifacts have ambiguous names or a different source run") end
  ' "$metadata" > "${work_dir}/selected-artifacts.json"
  if [[ "$(jq length "${work_dir}/selected-artifacts.json")" -eq 0 ]]; then
    return
  fi
  mkdir -p "$downloaded"
  while IFS= read -r name; do
    run_step "download-${name}" gh run download "$source_run" --repo "$repository" \
      --name "$name" --dir "${downloaded}/${name}"
  done < <(jq -r '.[].name' "${work_dir}/selected-artifacts.json")
  while IFS= read -r -d '' asset; do
    name="${asset##*/}"
    if ! [[ "$name" =~ ^[A-Za-z0-9_.+-]+$ ]]; then
      echo "Error: Unexpected source artifact filename" >&2
      exit 1
    fi
    if [[ -f "${assets}/${name}" ]] && ! cmp -s "$asset" "${assets}/${name}"; then
      echo "Error: Duplicate source artifact content mismatch: ${name}" >&2
      exit 1
    fi
    cp "$asset" "${assets}/${name}"
  done < <(find "$downloaded" -type f \
    \( -name '*.whl' -o -name '*.tar.gz' -o -name '*.sigstore' -o -name '*.intoto.jsonl' \) -print0)
}

validate_packages() {
  local validation="${work_dir}/matrix"
  mkdir -p "${validation}/dist"
  cp "$assets"/*.whl "${validation}/dist/"
  (
    cd "$validation"
    PUBLISH_WHEEL_VERSION="$PACKAGE_VERSION" PUBLISH_WHEEL_MATRIX=stable \
      bash "${script_dir}/validate-wheel-artifacts.bash" "${work_dir}/wheel-manifest.tsv"
  )
  if [[ ! -s "${assets}/nautilus_trader-${PACKAGE_VERSION}.tar.gz" ]]; then
    echo "Error: Missing source distribution for ${PACKAGE_VERSION}" >&2
    exit 1
  fi
  for package in "$assets"/*.tar.gz; do
    if [[ "${package##*/}" != "nautilus_trader-${PACKAGE_VERSION}.tar.gz" ]]; then
      echo "Error: Unexpected source distribution: ${package##*/}" >&2
      exit 1
    fi
  done
}

preserve_manifest() {
  local name=$1
  if [[ -f "${remote}/${name}" ]]; then
    jq -S 'del(.generated_at)' "${remote}/${name}" > "${work_dir}/existing-manifest.json"
    jq -S 'del(.generated_at)' "${assets}/${name}" > "${work_dir}/expected-manifest.json"
    if ! cmp -s "${work_dir}/existing-manifest.json" "${work_dir}/expected-manifest.json"; then
      echo "Error: Existing release manifest content mismatch: ${name}" >&2
      exit 1
    fi
    cp "${remote}/${name}" "${assets}/${name}"
  fi
}

recheck_source() {
  gh api "repos/${repository}/actions/runs/${source_run}" > "${work_dir}/source-current.json"
  jq -e --arg sha "$source_sha" --argjson attempt "$source_attempt" '
    .head_sha == $sha and .run_attempt == $attempt and .status == "completed"
  ' "${work_dir}/source-current.json" > /dev/null || {
    echo "Error: Source run changed during recovery" >&2
    exit 1
  }
}

main "$@"
