#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
AWS_REGION="${TOKENMM_AWS_REGION:-ap-southeast-1}"
DB_INSTANCE_ID="${TOKENMM_TELEMETRY_DB_INSTANCE_ID:-nautilus-tokenmm-telemetry}"
DB_NAME="${NAUTILUS_TELEMETRY_PG_DATABASE:-nautilus_telemetry}"
DB_SCHEMA="${NAUTILUS_TELEMETRY_PG_SCHEMA:-telemetry}"
DB_PORT="${NAUTILUS_TELEMETRY_PG_PORT:-5432}"
DB_ENGINE="${TOKENMM_TELEMETRY_DB_ENGINE:-postgres}"
DB_VERSION="${TOKENMM_TELEMETRY_DB_ENGINE_VERSION:-16.13}"
DB_INSTANCE_CLASS="${TOKENMM_TELEMETRY_DB_INSTANCE_CLASS:-db.t4g.medium}"
ALLOCATED_STORAGE="${TOKENMM_TELEMETRY_DB_ALLOCATED_STORAGE:-100}"
MAX_ALLOCATED_STORAGE="${TOKENMM_TELEMETRY_DB_MAX_ALLOCATED_STORAGE:-500}"
BACKUP_RETENTION_DAYS="${TOKENMM_TELEMETRY_DB_BACKUP_RETENTION_DAYS:-7}"
SECRET_ID="${NAUTILUS_TELEMETRY_PG_SECRET_ID:-/nautilus/tokenmm/telemetry/postgres}"
HOST_ENV_PATH="${TOKENMM_TELEMETRY_HOST_ENV_PATH:-/etc/flux/common.env}"
DRY_RUN=0
APPLY_HOST_ENV=0

require_cmd() {
  local name="$1"
  command -v "${name}" > /dev/null 2>&1 || {
    echo "[tokenmm-telemetry-rds] missing required command: ${name}" >&2
    exit 1
  }
}

parse_args() {
  while (($#)); do
    case "$1" in
      --dry-run)
        DRY_RUN=1
        ;;
      --apply-host-env)
        APPLY_HOST_ENV=1
        ;;
      *)
        echo "[tokenmm-telemetry-rds] unsupported argument: $1" >&2
        exit 1
        ;;
    esac
    shift
  done
}

run_cmd() {
  echo "+ $*" >&2
  if [[ "${DRY_RUN}" == "1" ]]; then
    return 0
  fi
  "$@"
}

metadata_token() {
  curl -fsS -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600"
}

metadata_get() {
  local token="$1"
  local path="$2"
  curl -fsS -H "X-aws-ec2-metadata-token: ${token}" "http://169.254.169.254/latest/${path}"
}

read_secret_json() {
  aws secretsmanager get-secret-value \
    --region "${AWS_REGION}" \
    --secret-id "${SECRET_ID}" \
    --query SecretString \
    --output text 2> /dev/null || true
}

upsert_env_file() {
  local env_path="$1"
  shift
  mkdir -p "$(dirname "${env_path}")"
  touch "${env_path}"
  local tmp
  tmp="$(mktemp)"
  cp "${env_path}" "${tmp}"
  local pair key value
  for pair in "$@"; do
    key="${pair%%=*}"
    value="${pair#*=}"
    if grep -q "^${key}=" "${tmp}"; then
      sed -i "s|^${key}=.*|${key}=${value}|" "${tmp}"
    else
      printf '%s=%s\n' "${key}" "${value}" >> "${tmp}"
    fi
  done
  install -m 0640 "${tmp}" "${env_path}"
  rm -f "${tmp}"
}

main() {
  parse_args "$@"
  require_cmd aws
  require_cmd jq
  require_cmd openssl
  require_cmd curl

  local imds_token instance_id identity_doc az vpc_id
  imds_token="$(metadata_token)"
  instance_id="$(metadata_get "${imds_token}" "meta-data/instance-id")"
  identity_doc="$(metadata_get "${imds_token}" "dynamic/instance-identity/document")"
  az="$(printf '%s' "${identity_doc}" | jq -r '.availabilityZone')"

  local instance_desc subnet_id
  instance_desc="$(aws ec2 describe-instances --region "${AWS_REGION}" --instance-ids "${instance_id}")"
  subnet_id="$(printf '%s' "${instance_desc}" | jq -r '.Reservations[0].Instances[0].SubnetId')"
  vpc_id="$(printf '%s' "${instance_desc}" | jq -r '.Reservations[0].Instances[0].VpcId')"
  mapfile -t host_security_groups < <(printf '%s' "${instance_desc}" | jq -r '.Reservations[0].Instances[0].SecurityGroups[].GroupId')
  mapfile -t subnet_ids < <(aws ec2 describe-subnets --region "${AWS_REGION}" --filters "Name=vpc-id,Values=${vpc_id}" "Name=state,Values=available" | jq -r '.Subnets[].SubnetId')

  local db_sg_name="nautilus-tokenmm-telemetry-rds"
  local db_sg_id=""
  db_sg_id="$(
    aws ec2 describe-security-groups --region "${AWS_REGION}" --filters "Name=vpc-id,Values=${vpc_id}" "Name=group-name,Values=${db_sg_name}" | jq -r '.SecurityGroups[0].GroupId // empty'
  )"
  if [[ -z "${db_sg_id}" ]]; then
    if [[ "${DRY_RUN}" == "1" ]]; then
      run_cmd aws ec2 create-security-group --region "${AWS_REGION}" --vpc-id "${vpc_id}" --group-name "${db_sg_name}" --description "TokenMM telemetry RDS access"
      db_sg_id="sg-dryrun-tokenmmtelemetry"
    else
      db_sg_id="$(run_cmd aws ec2 create-security-group --region "${AWS_REGION}" --vpc-id "${vpc_id}" --group-name "${db_sg_name}" --description "TokenMM telemetry RDS access" | jq -r '.GroupId')"
    fi
  fi

  local sg_id
  for sg_id in "${host_security_groups[@]}"; do
    run_cmd aws ec2 authorize-security-group-ingress \
      --region "${AWS_REGION}" \
      --group-id "${db_sg_id}" \
      --ip-permissions "IpProtocol=tcp,FromPort=${DB_PORT},ToPort=${DB_PORT},UserIdGroupPairs=[{GroupId=${sg_id}}]" >/dev/null 2>&1 || true
  done

  local subnet_group_name="${DB_INSTANCE_ID}-db-subnet-group"
  if ! aws rds describe-db-subnet-groups --region "${AWS_REGION}" --db-subnet-group-name "${subnet_group_name}" > /dev/null 2>&1; then
    run_cmd aws rds create-db-subnet-group \
      --region "${AWS_REGION}" \
      --db-subnet-group-name "${subnet_group_name}" \
      --db-subnet-group-description "TokenMM telemetry db-subnet-group" \
      --subnet-ids "${subnet_ids[@]}" > /dev/null
  fi

  local secret_json secret_payload secret_password secret_username
  secret_json="$(read_secret_json)"
  secret_payload="${secret_json}"
  if [[ -z "${secret_payload}" ]]; then
    secret_payload='{}'
  fi
  secret_username="$(printf '%s' "${secret_payload}" | jq -r '.username // "nautilus_tokenmm"')"
  secret_password="$(printf '%s' "${secret_payload}" | jq -r '.password // empty')"
  if [[ -z "${secret_password}" ]]; then
    secret_password="$(openssl rand -base64 30 | tr -d '\n' | tr '/+' 'AZ')"
  fi

  if [[ -z "${secret_json}" ]]; then
    run_cmd aws secretsmanager create-secret \
      --region "${AWS_REGION}" \
      --name "${SECRET_ID}" \
      --secret-string "{\"host\":\"\",\"port\":${DB_PORT},\"database\":\"${DB_NAME}\",\"schema\":\"${DB_SCHEMA}\",\"username\":\"${secret_username}\",\"password\":\"${secret_password}\",\"sslmode\":\"require\"}" > /dev/null
  fi

  local db_desc db_host
  db_desc="$(aws rds describe-db-instances --region "${AWS_REGION}" --db-instance-identifier "${DB_INSTANCE_ID}" 2> /dev/null || true)"
  if [[ -z "${db_desc}" ]]; then
    run_cmd aws rds create-db-instance \
      --region "${AWS_REGION}" \
      --db-instance-identifier "${DB_INSTANCE_ID}" \
      --engine "${DB_ENGINE}" \
      --engine-version "${DB_VERSION}" \
      --db-instance-class "${DB_INSTANCE_CLASS}" \
      --allocated-storage "${ALLOCATED_STORAGE}" \
      --max-allocated-storage "${MAX_ALLOCATED_STORAGE}" \
      --storage-type gp3 \
      --storage-encrypted \
      --no-publicly-accessible \
      --backup-retention-period "${BACKUP_RETENTION_DAYS}" \
      --master-username "${secret_username}" \
      --master-user-password "${secret_password}" \
      --db-name "${DB_NAME}" \
      --port "${DB_PORT}" \
      --db-subnet-group-name "${subnet_group_name}" \
      --vpc-security-group-ids "${db_sg_id}" \
      --multi-az \
      --copy-tags-to-snapshot >/dev/null
    if [[ "${DRY_RUN}" == "1" ]]; then
      db_desc='{"DBInstances":[{"Endpoint":{"Address":"nautilus-tokenmm-telemetry.dry-run.ap-southeast-1.rds.amazonaws.com"}}]}'
    else
      aws rds wait db-instance-available --region "${AWS_REGION}" --db-instance-identifier "${DB_INSTANCE_ID}"
      db_desc="$(aws rds describe-db-instances --region "${AWS_REGION}" --db-instance-identifier "${DB_INSTANCE_ID}")"
    fi
  fi

  db_host="$(printf '%s' "${db_desc}" | jq -r '.DBInstances[0].Endpoint.Address // empty')"

  if [[ "${DRY_RUN}" != "1" ]]; then
    aws secretsmanager put-secret-value \
      --region "${AWS_REGION}" \
      --secret-id "${SECRET_ID}" \
      --secret-string "{\"host\":\"${db_host}\",\"port\":${DB_PORT},\"database\":\"${DB_NAME}\",\"schema\":\"${DB_SCHEMA}\",\"username\":\"${secret_username}\",\"password\":\"${secret_password}\",\"sslmode\":\"require\"}" > /dev/null
  fi

  cat <<EOF
TOKENMM_AWS_REGION=${AWS_REGION}
NAUTILUS_TELEMETRY_PG_SECRET_ID=${SECRET_ID}
NAUTILUS_TELEMETRY_PG_HOST=${db_host}
NAUTILUS_TELEMETRY_PG_PORT=${DB_PORT}
NAUTILUS_TELEMETRY_PG_DATABASE=${DB_NAME}
NAUTILUS_TELEMETRY_PG_SCHEMA=${DB_SCHEMA}
NAUTILUS_TELEMETRY_PG_SSLMODE=require
EOF

  if [[ "${APPLY_HOST_ENV}" == "1" && "${DRY_RUN}" != "1" ]]; then
    upsert_env_file \
      "${HOST_ENV_PATH}" \
      "TOKENMM_AWS_REGION=${AWS_REGION}" \
      "NAUTILUS_TELEMETRY_PG_SECRET_ID=${SECRET_ID}" \
      "NAUTILUS_TELEMETRY_PG_HOST=${db_host}" \
      "NAUTILUS_TELEMETRY_PG_PORT=${DB_PORT}" \
      "NAUTILUS_TELEMETRY_PG_DATABASE=${DB_NAME}" \
      "NAUTILUS_TELEMETRY_PG_SCHEMA=${DB_SCHEMA}" \
      "NAUTILUS_TELEMETRY_PG_SSLMODE=require"
  fi
}

main "$@"
