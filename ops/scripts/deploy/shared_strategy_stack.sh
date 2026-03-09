#!/usr/bin/env bash

strategy_stack_install_base_units() {
  local root_dir="$1"
  local systemd_dir="$2"
  local env_dir="$3"
  local common_env_source="$4"
  local common_env_path="$5"

  install -d "${systemd_dir}" "${env_dir}"
  install -m 0644 "${root_dir}/deploy/systemd/flux@.service" "${systemd_dir}/flux@.service"
  if [[ ! -f "${common_env_path}" ]]; then
    install -m 0640 "${common_env_source}" "${common_env_path}"
  fi
}


strategy_stack_render_target() {
  local target_path="$1"
  local description="$2"
  shift 2

  {
    echo '[Unit]'
    echo "Description=${description}"
    local service_id
    for service_id in "$@"; do
      echo "Wants=flux@${service_id}.service"
    done
    echo 'After=network-online.target'
    echo
    echo '[Install]'
    echo 'WantedBy=multi-user.target'
  } > "${target_path}"
}


strategy_stack_write_env() {
  local env_path="$1"
  local pulse_description="$2"
  local group_key="$3"
  local group_label="$4"
  local group_order="$5"
  local cmd="$6"
  local port="${7:-}"
  local self_service_id="${8:-}"
  local pulse_enabled="${9:-1}"

  {
    echo "PULSE_ENABLED=${pulse_enabled}"
    echo "PULSE_DESCRIPTION=${pulse_description}"
    echo "PULSE_GROUP_KEY=${group_key}"
    echo "PULSE_GROUP_LABEL=${group_label}"
    echo "PULSE_GROUP_ORDER=${group_order}"
    if [[ -n "${self_service_id}" ]]; then
      echo "PULSE_SELF_SERVICE_ID=${self_service_id}"
    fi
    if [[ -n "${port}" ]]; then
      echo "PORT=${port}"
    fi
    printf 'CMD="%s"\n' "${cmd}"
  } > "${env_path}"
}


strategy_stack_render_sudoers() {
  local run_as_user="$1"
  local sudoers_path="$2"
  shift 2

  {
    echo '# Allow the API service user to operate managed Flux services for Pulse.'
    echo 'Cmnd_Alias FLUX_PULSE = \'
    local service_id
    local count=$#
    local index=0
    for service_id in "$@"; do
      index=$((index + 1))
      local suffix=", \\"
      if [[ ${index} -eq ${count} ]]; then
        suffix=""
      fi
      echo "  /usr/bin/systemctl start flux@${service_id}.service, \\"
      echo "  /usr/bin/systemctl stop flux@${service_id}.service, \\"
      echo "  /usr/bin/systemctl restart flux@${service_id}.service, \\"
      echo "  /usr/bin/journalctl -u flux@${service_id}.service${suffix}"
    done
    echo
    echo "${run_as_user} ALL=(root) NOPASSWD: FLUX_PULSE"
  } > "${sudoers_path}"
}


strategy_stack_read_env_value() {
  local env_path="$1"
  local key="$2"
  local line=""

  while IFS= read -r line || [[ -n "${line}" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "${line}" ]] && continue
    [[ "${line}" == \#* ]] && continue
    [[ "${line}" != "${key}"=* ]] && continue

    local value="${line#*=}"
    if [[ ${#value} -ge 2 && "${value:0:1}" == "${value: -1}" ]]; then
      case "${value:0:1}" in
        '"'|"'")
          value="${value:1:${#value}-2}"
          ;;
      esac
    fi
    printf '%s\n' "${value}"
    return 0
  done < "${env_path}"

  return 1
}


strategy_stack_require_identifier() {
  local identifier="$1"
  local label="$2"

  if [[ ! "${identifier}" =~ ^[[:alnum:]][[:alnum:]_-]*$ ]]; then
    echo "[strategy-stack] invalid ${label}: ${identifier}" >&2
    return 1
  fi
}


strategy_stack_collect_pulse_service_ids() {
  local -n out_service_ids="$1"
  local env_dir="$2"
  out_service_ids=()

  [[ -d "${env_dir}" ]] || return 0

  local env_path=""
  while IFS= read -r env_path; do
    [[ -n "${env_path}" ]] || continue
    local pulse_enabled=""
    pulse_enabled="$(strategy_stack_read_env_value "${env_path}" "PULSE_ENABLED" || true)"
    if [[ "${pulse_enabled}" != "1" ]]; then
      continue
    fi
    local service_id
    service_id="$(basename "${env_path%.env}")"
    strategy_stack_require_identifier "${service_id}" "service ID from ${env_path}" || return 1
    out_service_ids+=("${service_id}")
  done < <(find "${env_dir}" -maxdepth 1 -type f -name '*.env' | sort)
}


strategy_stack_discover_strategy_ids() {
  local strategies_dir="$1"
  local excluded_template="${2:-*template*}"

  local strategy_file=""
  while IFS= read -r strategy_file; do
    [[ -n "${strategy_file}" ]] || continue
    local strategy_id="${strategy_file%.toml}"
    strategy_stack_require_identifier "${strategy_id}" "strategy ID from ${strategies_dir}/${strategy_file}" || return 1
    printf '%s\n' "${strategy_id}"
  done < <(
    find "${strategies_dir}" -maxdepth 1 -type f -name '*.toml' ! -name "${excluded_template}" -printf '%f\n' | sort
  )
}


strategy_stack_print_strategy_configs() {
  local strategies_dir="$1"
  local excluded_template="${2:-*template*}"

  find "${strategies_dir}" -maxdepth 1 -type f -name '*.toml' ! -name "${excluded_template}" | sort
}


strategy_stack_load_strategy_configs() {
  local -n out_configs="$1"
  local strategies_dir="$2"
  local excluded_template="${3:-*template*}"
  local log_prefix="${4:-[strategy-stack]}"
  local expected_nodes="${5:-0}"

  mapfile -t out_configs < <(
    strategy_stack_print_strategy_configs "${strategies_dir}" "${excluded_template}"
  )

  if ((${#out_configs[@]} == 0)); then
    echo "${log_prefix} no strategy configs found under ${strategies_dir}" >&2
    exit 1
  fi

  if [[ "${expected_nodes}" =~ ^[0-9]+$ ]] && [[ "${expected_nodes}" != "0" ]]; then
    if ((${#out_configs[@]} != expected_nodes)); then
      echo "${log_prefix} expected ${expected_nodes} strategy configs, found ${#out_configs[@]}" >&2
      exit 1
    fi
  fi
}


strategy_stack_print_install_hint() {
  local log_prefix="$1"
  local install_script_path="$2"

  echo "${log_prefix} install flux@ services with ${install_script_path}" >&2
}
