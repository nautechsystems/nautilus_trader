#!/bin/sh

set -eu

WORK_DIR=""
LOOKUP_TIMEOUT=15

main() {
  check_commands

  WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-outdated.XXXXXX")
  trap cleanup 0

  cargo_output="$WORK_DIR/cargo.txt"
  candidates="$WORK_DIR/candidates.tsv"
  uv_output="$WORK_DIR/uv.txt"

  run_and_capture "$WORK_DIR/cargo.status" "$cargo_output" \
    env CARGO_TERM_COLOR=never cargo upgrade --dry-run --incompatible
  run_and_capture "$WORK_DIR/uv.status" "$uv_output" \
    env NO_COLOR=1 uv tree --outdated --depth 1 --all-groups

  parse_cargo "$cargo_output" > "$candidates"
  parse_pypi "$uv_output" >> "$candidates"
  print_release_ages "$candidates"
}

check_commands() {
  for required_command in cargo uv curl jq awk mktemp tee cat; do
    command -v "$required_command" > /dev/null || {
      printf 'Required command not on PATH: %s\n' "$required_command" >&2
      exit 2
    }
  done
}

cleanup() {
  if [ -n "${WORK_DIR:-}" ] && [ -d "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR"
  fi
}

run_and_capture() (
  status_file=$1
  output_file=$2
  shift 2

  (
    set +e
    "$@"
    printf '%s\n' "$?" > "$status_file"
  ) 2>&1 | tee "$output_file"

  command_status=$(cat "$status_file")
  [ "$command_status" -eq 0 ] || exit "$command_status"
)

parse_cargo() {
  awk '
    $1 == "name" && $2 == "old" && $3 == "req" {
      in_table = 1
      next
    }
    in_table && $1 ~ /^=+$/ {
      next
    }
    in_table {
      if (NF < 4 || $4 !~ /^[0-9]/) {
        in_table = 0
        next
      }

      key = $1 SUBSEP $2 SUBSEP $4
      if (!seen[key]++) {
        printf "cargo\t%s\t%s\t%s\n", $1, $2, $4
      }
    }
  ' "$1"
}

parse_pypi() {
  awk '
    index($0, "(latest: v") {
      name = $2
      current = $3
      sub(/\[.*$/, "", name)
      sub(/^v/, "", current)

      latest = $0
      sub(/^.*\(latest: v/, "", latest)
      sub(/\).*$/, "", latest)

      key = name SUBSEP current SUBSEP latest
      if (!seen[key]++) {
        printf "pypi\t%s\t%s\t%s\n", name, current, latest
      }
    }
  ' "$1"
}

print_release_ages() (
  candidates=$1
  lookup_failures=0
  color_red=
  color_orange=
  color_reset=

  if [ -t 1 ] && [ -z "${NO_COLOR+x}" ]; then
    color_red=$(printf '\033[0;31m')
    color_orange=$(printf '\033[38;5;208m')
    color_reset=$(printf '\033[0m')
  fi

  printf '\nDependency release ages:\n'

  if [ ! -s "$candidates" ]; then
    printf '  All dependencies up to date\n'
    return
  fi

  now=$(jq -nr 'now | floor')
  printf '%-7s %-32s %-14s %-14s %-20s %s\n' \
    "source" "package" "current" "latest" "released (UTC)" "age"

  tab=$(printf '\t')
  while IFS="$tab" read -r ecosystem name current latest; do
    if published=$(release_time "$ecosystem" "$name" "$latest") &&
      age=$(format_age "$published" "$now"); then
      released=${published%%.*}
      released=${released%Z}
      released=${released%+00:00}
      release_date=${released%%T*}
      release_clock=${released#*T}
      released="$release_date $release_clock"
    else
      released="unknown"
      age="unknown"
      lookup_failures=$((lookup_failures + 1))
    fi

    age_display=$age
    case "$age" in
      *d*)
        age_days=${age%%d*}
        if [ "$age_days" -eq 0 ]; then
          age_display="${color_red}${age}${color_reset}"
        elif [ "$age_days" -lt 3 ]; then
          age_display="${color_orange}${age}${color_reset}"
        fi
        ;;
    esac

    printf '%-7s %-32s %-14s %-14s %-20s %s\n' \
      "$ecosystem" "$name" "$current" "$latest" "$released" "$age_display"
  done < "$candidates"

  if [ "$lookup_failures" -gt 0 ]; then
    printf '\nWarning: %s release age lookup(s) failed.\n' "$lookup_failures" >&2
  fi
)

release_time() (
  ecosystem=$1
  name=$2
  version=$3

  case "$ecosystem" in
    cargo)
      url="https://crates.io/api/v1/crates/${name}/${version}"
      response=$(curl -fsSL --max-time "$LOOKUP_TIMEOUT" \
        -A "nautilus-outdated/1.0" "$url" 2> /dev/null) || return 1
      printf '%s' "$response" | jq -er '.version.created_at // empty'
      ;;
    pypi)
      url="https://pypi.org/pypi/${name}/${version}/json"
      response=$(curl -fsSL --max-time "$LOOKUP_TIMEOUT" \
        -A "nautilus-outdated/1.0" "$url" 2> /dev/null) || return 1
      printf '%s' "$response" |
        jq -er '[.urls[]? | .upload_time_iso_8601] | min // empty'
      ;;
    *)
      return 1
      ;;
  esac
)

format_age() (
  published=$1
  now=$2

  published_seconds=$(jq -ner --arg timestamp "$published" '
    $timestamp
    | sub("\\+00:00$"; "Z")
    | sub("\\.[0-9]+Z$"; "Z")
    | fromdateiso8601
    | floor
  ' 2> /dev/null) || return 1
  age_seconds=$((now - published_seconds))

  if [ "$age_seconds" -lt 0 ]; then
    printf 'future'
    return
  fi

  days=$((age_seconds / 86400))
  hours=$(((age_seconds % 86400) / 3600))
  printf '%3dd %2dh' "$days" "$hours"
)

main "$@"
