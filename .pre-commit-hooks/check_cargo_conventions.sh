#!/usr/bin/env bash
# Enforces Cargo.toml conventions:
# 1. Dependencies within groups (separated by blank lines) must be alphabetically ordered
# 2. Sections must be in standard order: package, lints, lib, features, package.metadata.docs.rs,
#    dependencies, dev-dependencies, build-dependencies, bench, bin, example, test
# 3. Crates with [lib] or [[bin]] must have [lints] workspace = true
# 4. All [[bin]] and [[example]] sections must have doc = false
# 5. [package] section must have required fields in correct order
# 6. [lib] crate-type must use order: rlib, staticlib, cdylib
# 7. All [workspace.dependencies] must be used by at least one crate
# 8. Related dependency versions must be aligned (e.g., capnp/capnpc)
# 9. Adapter dependencies section should only contain deps used exclusively by adapters
#
# Dependency groups are typically organized as:
# - Internal nautilus-* dependencies
# - External dependencies
# - Optional dependencies
# Each group is separated by a blank line

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping Cargo convention checks"
  exit 0
fi

RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "Checking Cargo.toml conventions..."

VIOLATIONS=0

# Check 1: Dependency ordering within groups
# shellcheck disable=SC2016
dep_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" 2> /dev/null | sort | xargs awk '
BEGIN {
  in_deps = 0
  section = ""
  prev_name = ""
  prev_line = 0
}

/^\[+[a-zA-Z0-9._-]+\]+$/ {
  prev_name = ""
  prev_line = 0
  gsub(/^\[+|\]+$/, "", $0)
  if ($0 == "dependencies" || $0 == "dev-dependencies" || $0 == "build-dependencies" || $0 == "workspace.dependencies") {
    in_deps = 1
    section = $0
  } else {
    in_deps = 0
    section = ""
  }
  next
}

in_deps && /^[[:space:]]*$/ {
  prev_name = ""
  prev_line = 0
  next
}

in_deps && /^[[:space:]]*#/ { next }

in_deps && /^[a-zA-Z0-9_-]+[[:space:]]*[.=]/ {
  match($0, /^[a-zA-Z0-9_-]+/)
  name = substr($0, RSTART, RLENGTH)
  name_lower = tolower(name)

  if (prev_name != "" && name_lower < tolower(prev_name)) {
    printf "  %s:%d [%s] \047%s\047 should come before \047%s\047 (line %d)\n", FILENAME, NR, section, name, prev_name, prev_line
  }

  prev_name = name
  prev_line = NR
}
' 2>&1) || true

if [[ -n "$dep_violations" ]]; then
  echo -e "${RED}Dependency ordering violations:${NC}"
  echo "$dep_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$dep_violations" | wc -l)))
fi

# Check 2: Section ordering
# Expected order (not all required): package, lints, lib, features, package.metadata.docs.rs,
#                                    dependencies, dev-dependencies, build-dependencies, bench, bin, example, test
section_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" crates/ 2> /dev/null | while read -r file; do
  awk '
  BEGIN {
    # Manually assign order indices
    order_map["package"] = 1
    order_map["lints"] = 2
    order_map["lib"] = 3
    order_map["features"] = 4
    order_map["package.metadata.docs.rs"] = 5
    order_map["dependencies"] = 6
    order_map["dev-dependencies"] = 7
    order_map["build-dependencies"] = 8
    order_map["bench"] = 9
    order_map["bin"] = 10
    order_map["example"] = 11
    order_map["test"] = 12
    prev_section = ""
    prev_idx = 0
  }

  /^\[+[a-zA-Z0-9._-]+\]+$/ {
    section = $0
    gsub(/^\[+|\]+$/, "", section)

    if (section in order_map) {
      idx = order_map[section]
      if (prev_idx > 0 && idx < prev_idx) {
        printf "  %s:%d [%s] should come before [%s]\n", FILENAME, NR, section, prev_section
      }
      prev_section = section
      prev_idx = idx
    }
  }
  ' "$file"
done) || true

if [[ -n "$section_violations" ]]; then
  echo -e "${RED}Section ordering violations:${NC}"
  echo "$section_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$section_violations" | wc -l)))
fi

# Check 3: [lints] workspace = true required for crates with [lib] or [[bin]]
# Exclude placeholder manifests (crates/Cargo.toml)
lints_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" crates/ 2> /dev/null | while read -r file; do
  # Skip the placeholder manifest
  [[ "$file" == "crates/Cargo.toml" ]] && continue

  has_lib_or_bin=$(grep -E '^\[lib\]|\[\[bin\]\]' "$file" 2> /dev/null || true)
  if [[ -z "$has_lib_or_bin" ]]; then
    continue
  fi

  # Check for [lints] section with workspace = true
  has_workspace_lints=$(awk '
    /^\[lints\]/ { in_lints = 1; next }
    /^\[/ { in_lints = 0 }
    in_lints && /^workspace[[:space:]]*=[[:space:]]*true/ { found = 1 }
    END { if (found) print "yes" }
  ' "$file")

  if [[ -z "$has_workspace_lints" ]]; then
    echo "  $file: missing [lints] workspace = true"
  fi
done) || true

if [[ -n "$lints_violations" ]]; then
  echo -e "${RED}Missing [lints] section:${NC}"
  echo "$lints_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$lints_violations" | wc -l)))
fi

# Check 4: [[bin]] and [[example]] must have doc = false
doc_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" crates/ 2> /dev/null | while read -r file; do
  awk '
  function check_pending() {
    if (section_line > 0 && !has_doc_false) {
      printf "  %s:%d [[%s]] missing doc = false\n", FILENAME, section_line, section_type
    }
  }

  /^\[\[bin\]\]/ || /^\[\[example\]\]/ {
    # Check previous section before starting new one
    check_pending()

    section_type = $0
    gsub(/^\[\[|\]\]$/, "", section_type)
    section_line = NR
    has_doc_false = 0
    next
  }

  section_line > 0 && /^doc[[:space:]]*=[[:space:]]*false/ {
    has_doc_false = 1
  }

  section_line > 0 && /^\[/ && !/^\[\[bin\]\]/ && !/^\[\[example\]\]/ {
    check_pending()
    section_line = 0
    has_doc_false = 0
  }

  END {
    check_pending()
  }
  ' "$file"
done) || true

if [[ -n "$doc_violations" ]]; then
  echo -e "${RED}Missing doc = false:${NC}"
  echo "$doc_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$doc_violations" | wc -l)))
fi

# Check 5: [package] section field ordering
# Required order: name, readme, version.workspace, edition.workspace, rust-version.workspace,
#                 authors.workspace, license.workspace, description, categories.workspace,
#                 keywords.workspace, documentation.workspace, repository.workspace, homepage.workspace
# Optional fields (publish, build, include) can appear after homepage.workspace
package_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" crates/ 2> /dev/null | while read -r file; do
  # Skip placeholder manifest
  [[ "$file" == "crates/Cargo.toml" ]] && continue

  awk '
  BEGIN {
    # Define expected field order
    field_order["name"] = 1
    field_order["readme"] = 2
    field_order["version.workspace"] = 3
    field_order["edition.workspace"] = 4
    field_order["rust-version.workspace"] = 5
    field_order["authors.workspace"] = 6
    field_order["license.workspace"] = 7
    field_order["description"] = 8
    field_order["categories.workspace"] = 9
    field_order["keywords.workspace"] = 10
    field_order["documentation.workspace"] = 11
    field_order["repository.workspace"] = 12
    field_order["homepage.workspace"] = 13

    # Required fields
    required["name"] = 1
    required["version.workspace"] = 1
    required["edition.workspace"] = 1
    required["rust-version.workspace"] = 1
    required["authors.workspace"] = 1
    required["license.workspace"] = 1
    required["description"] = 1
    required["categories.workspace"] = 1
    required["keywords.workspace"] = 1
    required["documentation.workspace"] = 1
    required["repository.workspace"] = 1
    required["homepage.workspace"] = 1

    in_package = 0
    prev_field = ""
    prev_idx = 0
  }

  /^\[package\]/ {
    in_package = 1
    next
  }

  in_package && /^\[/ {
    in_package = 0
    # Check for missing required fields
    for (f in required) {
      if (!(f in found)) {
        printf "  %s: [package] missing required field: %s\n", FILENAME, f
      }
    }
    next
  }

  in_package && /^[a-zA-Z]/ {
    # Extract field name (handle both "field = value" and "field.workspace = true")
    match($0, /^[a-zA-Z0-9._-]+/)
    field = substr($0, RSTART, RLENGTH)

    # Normalize field name for ordering check
    if (field ~ /\.workspace$/) {
      norm_field = field
    } else if ($0 ~ /\.workspace[[:space:]]*=/) {
      norm_field = field ".workspace"
    } else {
      norm_field = field
    }

    found[norm_field] = 1

    if (norm_field in field_order) {
      idx = field_order[norm_field]
      if (prev_idx > 0 && idx < prev_idx) {
        printf "  %s:%d [package] field \047%s\047 should come before \047%s\047\n", FILENAME, NR, norm_field, prev_field
      }
      prev_field = norm_field
      prev_idx = idx
    }
  }

  END {
    if (in_package) {
      for (f in required) {
        if (!(f in found)) {
          printf "  %s: [package] missing required field: %s\n", FILENAME, f
        }
      }
    }
  }
  ' "$file"
done) || true

if [[ -n "$package_violations" ]]; then
  echo -e "${RED}[package] section violations:${NC}"
  echo "$package_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$package_violations" | wc -l)))
fi

# Check 6: [lib] crate-type ordering (rlib, staticlib, cdylib)
crate_type_violations=$(rg --files -g "Cargo.toml" --glob "!target/*" crates/ 2> /dev/null | while read -r file; do
  grep -E '^crate-type[[:space:]]*=' "$file" 2> /dev/null | while read -r line; do
    # Check if the order is correct: rlib before staticlib before cdylib
    if echo "$line" | grep -q 'cdylib.*rlib\|cdylib.*staticlib\|staticlib.*rlib'; then
      echo "  $file: crate-type should use order [\"rlib\", \"staticlib\", \"cdylib\"]"
    fi
  done
done) || true

if [[ -n "$crate_type_violations" ]]; then
  echo -e "${RED}crate-type ordering violations:${NC}"
  echo "$crate_type_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$crate_type_violations" | wc -l)))
fi

# Check 7: Unused workspace dependencies
# Extract dependency names from [workspace.dependencies] and check if any crate uses them
if [[ -f "Cargo.toml" ]]; then
  unused_workspace_deps=$(awk '
  BEGIN { in_ws_deps = 0 }

  /^\[workspace\.dependencies\]/ { in_ws_deps = 1; next }
  /^\[/ && !/^\[workspace\.dependencies\]/ { in_ws_deps = 0 }

  # Match actual dependencies (lines with version, path, or workspace reference)
  in_ws_deps && /^[a-zA-Z][a-zA-Z0-9_-]*[[:space:]]*[.=]/ {
    match($0, /^[a-zA-Z][a-zA-Z0-9_-]*/)
    dep = substr($0, RSTART, RLENGTH)

    # Skip dev tools (defined for CI, not actual code dependencies)
    if (dep ~ /^cargo-/ || dep == "lychee") next

    # Skip top-level workspace members (not dependencies of other crates)
    if (dep == "nautilus-backtest" || dep == "nautilus-cli" || dep == "nautilus-pyo3") next

    print dep
  }
  ' Cargo.toml | while read -r dep; do
    # Check if any crate uses this dependency with workspace = true
    if ! grep -rq "^${dep}[[:space:]]*=" crates/*/Cargo.toml crates/adapters/*/Cargo.toml 2> /dev/null; then
      echo "  Cargo.toml: [workspace.dependencies] '$dep' is not used by any crate"
    fi
  done) || true

  if [[ -n "$unused_workspace_deps" ]]; then
    echo -e "${RED}Unused workspace dependencies:${NC}"
    echo "$unused_workspace_deps"
    echo
    VIOLATIONS=$((VIOLATIONS + $(echo "$unused_workspace_deps" | wc -l)))
  fi
fi

# Check 8: Related dependency version alignment
# Some dependencies must have matching versions (e.g., runtime and compiler crates)
version_alignment_violations=""

if [[ -f "Cargo.toml" ]]; then
  # Helper to extract version from Cargo.toml dependency line
  # Handles plain versions and common prefixes (^, =, ~, >=, etc.)
  get_version() {
    grep -E "^$1[[:space:]]*=" Cargo.toml | head -1 | grep -oE '"[~^=<>]*[0-9]+\.[0-9]+(\.[0-9]+)?([-+][a-zA-Z0-9.]+)?"' | head -1 | sed 's/"//g; s/^[~^=<>]*//' || echo ""
  }

  # Helper to extract major.minor from version
  get_major_minor() {
    echo "$1" | cut -d. -f1,2
  }

  # capnp and capnpc must have exact same version
  capnp_ver=$(get_version "capnp")
  capnpc_ver=$(get_version "capnpc")
  if [[ -n "$capnp_ver" && -n "$capnpc_ver" && "$capnp_ver" != "$capnpc_ver" ]]; then
    version_alignment_violations+="  Cargo.toml: capnp ($capnp_ver) and capnpc ($capnpc_ver) versions must match"$'\n'
  fi

  # arrow and parquet must have same major.minor (from same arrow-rs release)
  arrow_ver=$(get_version "arrow")
  parquet_ver=$(get_version "parquet")
  if [[ -n "$arrow_ver" && -n "$parquet_ver" ]]; then
    arrow_mm=$(get_major_minor "$arrow_ver")
    parquet_mm=$(get_major_minor "$parquet_ver")
    if [[ "$arrow_mm" != "$parquet_mm" ]]; then
      version_alignment_violations+="  Cargo.toml: arrow ($arrow_ver) and parquet ($parquet_ver) major.minor versions must match"$'\n'
    fi
  fi

  # object_store must be compatible with datafusion (check datafusion's Cargo.toml when updating)
  # datafusion 51.x requires object_store ^0.12
  datafusion_ver=$(get_version "datafusion")
  object_store_ver=$(get_version "object_store")
  if [[ -n "$datafusion_ver" && -n "$object_store_ver" ]]; then
    df_major=$(echo "$datafusion_ver" | cut -d. -f1)
    os_mm=$(get_major_minor "$object_store_ver")
    # Known compatible pairs: datafusion 51.x -> object_store 0.12.x
    if [[ "$df_major" == "51" && "$os_mm" != "0.12" ]]; then
      version_alignment_violations+="  Cargo.toml: datafusion $datafusion_ver requires object_store 0.12.x (found $object_store_ver)"$'\n'
    fi
  fi

  # prost and tonic must be compatible with dydx-proto (check dydx-proto's Cargo.toml when updating)
  # dydx-proto 0.4.x requires prost ^0.13 and tonic ^0.13
  dydx_proto_ver=$(get_version "dydx-proto")
  prost_ver=$(get_version "prost")
  tonic_ver=$(get_version "tonic")
  if [[ -n "$dydx_proto_ver" ]]; then
    dydx_mm=$(get_major_minor "$dydx_proto_ver")
    # Known compatible pairs: dydx-proto 0.4.x -> prost 0.13.x, tonic 0.13.x
    if [[ "$dydx_mm" == "0.4" ]]; then
      if [[ -n "$prost_ver" ]]; then
        prost_mm=$(get_major_minor "$prost_ver")
        if [[ "$prost_mm" != "0.13" ]]; then
          version_alignment_violations+="  Cargo.toml: dydx-proto $dydx_proto_ver requires prost 0.13.x (found $prost_ver)"$'\n'
        fi
      fi
      if [[ -n "$tonic_ver" ]]; then
        tonic_mm=$(get_major_minor "$tonic_ver")
        if [[ "$tonic_mm" != "0.13" ]]; then
          version_alignment_violations+="  Cargo.toml: dydx-proto $dydx_proto_ver requires tonic 0.13.x (found $tonic_ver)"$'\n'
        fi
      fi
    fi
  fi
fi

if [[ -n "$version_alignment_violations" ]]; then
  echo -e "${RED}Version alignment violations:${NC}"
  echo "$version_alignment_violations"
  echo
  VIOLATIONS=$((VIOLATIONS + $(echo "$version_alignment_violations" | grep -c . || true)))
fi

# Check 9: Adapter dependencies should only be used by adapter crates
# Extract deps from "Adapter dependencies" section and verify they're not used by core crates
if [[ -f "Cargo.toml" ]]; then
  adapter_section_violations=""

  # Extract dependency names from the Adapter dependencies section
  adapter_section_deps=$(awk '
    /^# -+$/ { in_section = 0 }
    /^# Adapter dependencies/ { in_section = 1; next }
    in_section && /^[a-zA-Z][a-zA-Z0-9_-]*[[:space:]]*[.=]/ {
      match($0, /^[a-zA-Z][a-zA-Z0-9_-]*/)
      print substr($0, RSTART, RLENGTH)
    }
  ' Cargo.toml)

  # For each adapter dep, check if it's used by any non-adapter crate
  for dep in $adapter_section_deps; do
    # Check if used by core crates (not in adapters/)
    core_usage=$(find crates -maxdepth 2 -name "Cargo.toml" -not -path "*/adapters/*" -exec grep -l "^${dep}[[:space:]]*=" {} \; 2> /dev/null | head -1)
    if [[ -n "$core_usage" ]]; then
      adapter_section_violations+="  Cargo.toml: '$dep' in Adapter dependencies but used by core crate: $core_usage"$'\n'
    fi
  done

  if [[ -n "$adapter_section_violations" ]]; then
    echo -e "${RED}Adapter dependency section violations:${NC}"
    echo "$adapter_section_violations"
    echo -e "${YELLOW}Move these deps to Core dependencies section, or remove from core crates${NC}"
    echo
    VIOLATIONS=$((VIOLATIONS + $(echo "$adapter_section_violations" | grep -c . || true)))
  fi
fi

if [[ $VIOLATIONS -gt 0 ]]; then
  echo -e "${RED}Found $VIOLATIONS Cargo.toml convention violation(s)${NC}"
  echo
  echo -e "${YELLOW}To fix:${NC}"
  echo "  - Sort dependencies alphabetically within each group (groups separated by blank lines)"
  echo "  - Order sections: [package], [lints], [lib], [features], [package.metadata.docs.rs],"
  echo "    [dependencies], [dev-dependencies], [build-dependencies], [[bench]], [[bin]], [[example]]"
  echo "  - Add [lints] workspace = true after [package] for all crates"
  echo "  - Add doc = false to all [[bin]] and [[example]] sections"
  echo "  - [package] fields must be in order: name, readme, version.workspace, edition.workspace,"
  echo "    rust-version.workspace, authors.workspace, license.workspace, description,"
  echo "    categories.workspace, keywords.workspace, documentation.workspace, repository.workspace,"
  echo "    homepage.workspace, then optional fields (publish, build, include)"
  echo "  - crate-type must use order: [\"rlib\", \"staticlib\", \"cdylib\"]"
  echo "  - Remove unused dependencies from [workspace.dependencies] in root Cargo.toml"
  echo "  - Ensure related dependencies have matching versions (e.g., capnp and capnpc)"
  exit 1
fi

echo "✓ All Cargo.toml conventions are valid"
exit 0
