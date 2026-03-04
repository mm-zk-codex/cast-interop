#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
COMPOSE_FILE="${REPO_ROOT}/scripts/e2e/docker-compose.yml"

DEFAULT_SERVER_TAG="v0.16.0"
DEFAULT_SERVER_IMAGE_REPO="ghcr.io/matter-labs/zksync-os-server"
DEFAULT_ANVIL_IMAGE="ghcr.io/foundry-rs/foundry:v1.5.1"
LOCAL_CHAIN_VERSION="v31.0"
LOCAL_CHAIN_SCENARIO="multi_chain"
CHAIN_A_ID="6565"
CHAIN_B_ID="6566"
CHAIN_A_CONFIG="chain_6565.yaml"
CHAIN_B_CONFIG="chain_6566.yaml"

FLOW="bundle-relay"
KEEP_RUNNING=0
SERVER_TAG="${DEFAULT_SERVER_TAG}"
WORK_DIR=""
PROJECT_NAME=""

usage() {
  cat <<EOF
Usage: $(basename "$0") [--flow bundle-relay] [--server-tag TAG] [--keep-running]

Starts a disposable Docker E2E environment for cast-interop and runs the selected
integration test flow.

Options:
  --flow FLOW         Flow to run. Currently supported: bundle-relay
  --server-tag TAG    zksync-os-server tag to use (default: ${DEFAULT_SERVER_TAG})
  --keep-running      Leave the docker compose stack up after the script exits
  --help              Show this help message
EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

log() {
  printf '[e2e] %s\n' "$*"
}

compose() {
  docker compose \
    --project-name "${PROJECT_NAME}" \
    --env-file "${WORK_DIR}/compose.env" \
    -f "${COMPOSE_FILE}" \
    "$@"
}

cleanup() {
  local exit_code="$?"

  if [[ -n "${WORK_DIR}" && -d "${WORK_DIR}" ]]; then
    mkdir -p "${WORK_DIR}/logs"
    if [[ -n "${PROJECT_NAME}" ]]; then
      for service in anvil zksync-a zksync-b; do
        compose logs --no-color "${service}" >"${WORK_DIR}/logs/${service}.log" 2>&1 || true
      done
      compose ps >"${WORK_DIR}/logs/compose-ps.txt" 2>&1 || true
      if [[ "${KEEP_RUNNING}" -eq 0 ]]; then
        compose down --remove-orphans --volumes >/dev/null 2>&1 || true
      fi
    fi
    log "artifacts: ${WORK_DIR}"
    if [[ "${KEEP_RUNNING}" -eq 1 ]]; then
      log "docker compose stack left running because --keep-running was set"
    fi
  fi

  exit "${exit_code}"
}

prepare_workspace() {
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/cast-interop-e2e.XXXXXX")"
  mkdir -p \
    "${WORK_DIR}/source" \
    "${WORK_DIR}/runtime/anvil" \
    "${WORK_DIR}/runtime/chain-a/local-chains/${LOCAL_CHAIN_VERSION}" \
    "${WORK_DIR}/runtime/chain-b/local-chains/${LOCAL_CHAIN_VERSION}" \
    "${WORK_DIR}/logs"
  PROJECT_NAME="cast-interop-e2e-$(date +%s)-$$"
}

write_compose_env() {
  cat >"${WORK_DIR}/compose.env" <<EOF
E2E_ANVIL_DIR=${WORK_DIR}/runtime/anvil
E2E_CHAIN_A_DIR=${WORK_DIR}/runtime/chain-a
E2E_CHAIN_B_DIR=${WORK_DIR}/runtime/chain-b
ZKSYNC_OS_SERVER_IMAGE=${ZKSYNC_OS_SERVER_IMAGE}
ANVIL_IMAGE=${ANVIL_IMAGE}
ANVIL_PORT=8545
RPC_A_PORT=3050
RPC_B_PORT=3051
EOF
}

copy_required_assets() {
  local source_root="$1"

  cp "${source_root}/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json" \
    "${WORK_DIR}/runtime/chain-a/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json"
  cp "${source_root}/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json" \
    "${WORK_DIR}/runtime/chain-b/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json"
  cp "${source_root}/local-chains/${LOCAL_CHAIN_VERSION}/${LOCAL_CHAIN_SCENARIO}/${CHAIN_A_CONFIG}" \
    "${WORK_DIR}/runtime/chain-a/${CHAIN_A_CONFIG}"
  cp "${source_root}/local-chains/${LOCAL_CHAIN_VERSION}/${LOCAL_CHAIN_SCENARIO}/${CHAIN_B_CONFIG}" \
    "${WORK_DIR}/runtime/chain-b/${CHAIN_B_CONFIG}"
  cp "${source_root}/local-chains/${LOCAL_CHAIN_VERSION}/l1-state.json.gz" \
    "${WORK_DIR}/runtime/anvil/l1-state.json.gz"

  sed -i '/^general:$/a\  l1_rpc_url: http://anvil:8545' \
    "${WORK_DIR}/runtime/chain-a/${CHAIN_A_CONFIG}"
  sed -i '/^general:$/a\  l1_rpc_url: http://anvil:8545' \
    "${WORK_DIR}/runtime/chain-b/${CHAIN_B_CONFIG}"

  gzip -dc "${WORK_DIR}/runtime/anvil/l1-state.json.gz" \
    >"${WORK_DIR}/runtime/anvil/l1-state.json"
}

extract_assets_from_image() {
  local image_container
  image_container="$(docker create "${ZKSYNC_OS_SERVER_IMAGE}")"

  mkdir -p "${WORK_DIR}/source/image"

  local expected=(
    "/app/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json"
    "/app/local-chains/${LOCAL_CHAIN_VERSION}/${LOCAL_CHAIN_SCENARIO}/${CHAIN_A_CONFIG}"
    "/app/local-chains/${LOCAL_CHAIN_VERSION}/${LOCAL_CHAIN_SCENARIO}/${CHAIN_B_CONFIG}"
    "/app/local-chains/${LOCAL_CHAIN_VERSION}/l1-state.json.gz"
  )

  local source_path=""
  for source_path in "${expected[@]}"; do
    if ! docker cp "${image_container}:${source_path}" "${WORK_DIR}/source/image/" >/dev/null 2>&1; then
      docker rm -f "${image_container}" >/dev/null 2>&1 || true
      return 1
    fi
  done

  docker rm -f "${image_container}" >/dev/null 2>&1 || true

  mkdir -p "${WORK_DIR}/source/image/local-chains/${LOCAL_CHAIN_VERSION}"
  mv "${WORK_DIR}/source/image/genesis.json" \
    "${WORK_DIR}/source/image/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json"
  mv "${WORK_DIR}/source/image/${CHAIN_A_CONFIG}" \
    "${WORK_DIR}/source/image/local-chains/${LOCAL_CHAIN_VERSION}/${CHAIN_A_CONFIG}"
  mv "${WORK_DIR}/source/image/${CHAIN_B_CONFIG}" \
    "${WORK_DIR}/source/image/local-chains/${LOCAL_CHAIN_VERSION}/${CHAIN_B_CONFIG}"
  mv "${WORK_DIR}/source/image/l1-state.json.gz" \
    "${WORK_DIR}/source/image/local-chains/${LOCAL_CHAIN_VERSION}/l1-state.json.gz"

  copy_required_assets "${WORK_DIR}/source/image"
}

extract_assets_from_source_archive() {
  local archive_url="https://codeload.github.com/matter-labs/zksync-os-server/tar.gz/refs/tags/${SERVER_TAG}"
  local archive_path="${WORK_DIR}/source/zksync-os-server-${SERVER_TAG}.tar.gz"
  local extract_dir="${WORK_DIR}/source/archive"
  local root_dir

  log "downloading ${archive_url}"
  curl -fsSL "${archive_url}" -o "${archive_path}"
  mkdir -p "${extract_dir}"
  tar -xzf "${archive_path}" -C "${extract_dir}"
  root_dir="$(find "${extract_dir}" -mindepth 1 -maxdepth 1 -type d | head -n 1)"

  if [[ -z "${root_dir}" ]]; then
    echo "Failed to extract zksync-os-server source archive" >&2
    exit 1
  fi

  copy_required_assets "${root_dir}"
}

materialize_assets() {
  log "pulling ${ZKSYNC_OS_SERVER_IMAGE}"
  docker pull "${ZKSYNC_OS_SERVER_IMAGE}" >/dev/null

  if extract_assets_from_image; then
    log "using local-chains assets extracted from the image"
    return
  fi

  log "image did not contain the required ${LOCAL_CHAIN_VERSION} local-chains assets; falling back to source archive"
  extract_assets_from_source_archive
}

assert_runtime_files() {
  local required=(
    "${WORK_DIR}/runtime/anvil/l1-state.json"
    "${WORK_DIR}/runtime/chain-a/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json"
    "${WORK_DIR}/runtime/chain-b/local-chains/${LOCAL_CHAIN_VERSION}/genesis.json"
    "${WORK_DIR}/runtime/chain-a/${CHAIN_A_CONFIG}"
    "${WORK_DIR}/runtime/chain-b/${CHAIN_B_CONFIG}"
  )
  local path=""
  for path in "${required[@]}"; do
    if [[ ! -f "${path}" ]]; then
      echo "Missing runtime asset: ${path}" >&2
      exit 1
    fi
  done
}

relax_runtime_permissions() {
  chmod -R 777 "${WORK_DIR}/runtime"
}

rpc_ready() {
  local url="$1"
  local expected_chain_id="$2"
  local response=""

  response="$(curl -fsS \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
    "${url}" 2>/dev/null || true)"

  if [[ -z "${response}" ]]; then
    return 1
  fi

  python3 -c '
import json
import sys

payload = json.loads(sys.argv[1])
result = payload.get("result")
if result is None:
    raise SystemExit(1)
if int(result, 16) != int(sys.argv[2]):
    raise SystemExit(1)
' "${response}" "${expected_chain_id}" >/dev/null 2>&1
}

wait_for_rpc() {
  local url="$1"
  local expected_chain_id="$2"
  local name="$3"
  local attempts=120
  local index=0

  for ((index = 1; index <= attempts; index += 1)); do
    if rpc_ready "${url}" "${expected_chain_id}"; then
      log "${name} ready at ${url}"
      return
    fi
    sleep 1
  done

  echo "${name} did not become ready at ${url}" >&2
  exit 1
}

start_stack() {
  log "starting docker compose project ${PROJECT_NAME}"
  compose up -d --quiet-pull

  wait_for_rpc "http://127.0.0.1:8545" 31337 "anvil"
  wait_for_rpc "http://127.0.0.1:3050" "${CHAIN_A_ID}" "zksync-a"
  wait_for_rpc "http://127.0.0.1:3051" "${CHAIN_B_ID}" "zksync-b"
}

run_tests() {
  cargo build --bin cast-interop

  export CAST_INTEROP_L1_RPC="http://127.0.0.1:8545"
  export CAST_INTEROP_RPC_A="http://127.0.0.1:3050"
  export CAST_INTEROP_RPC_B="http://127.0.0.1:3051"
  export CAST_INTEROP_CHAIN_A_ID="${CHAIN_A_ID}"
  export CAST_INTEROP_CHAIN_B_ID="${CHAIN_B_ID}"
  export CAST_INTEROP_BIN="${REPO_ROOT}/target/debug/cast-interop"
  export CAST_INTEROP_E2E_PRIVATE_KEY="${CAST_INTEROP_E2E_PRIVATE_KEY:-${DEFAULT_PRIVATE_KEY}}"

  case "${FLOW}" in
  bundle-relay)
    cargo test --test bundle_relay parse_json_output_shape_examples -- --exact --nocapture
    cargo test --test bundle_relay bundle_relay_success \
      -- --ignored --exact --nocapture --test-threads=1
    cargo test --test bundle_relay bundle_relay_missing_receipt_fails \
      -- --ignored --exact --nocapture --test-threads=1
    ;;
  *)
    echo "Unsupported flow: ${FLOW}" >&2
    exit 1
    ;;
  esac
}

while [[ $# -gt 0 ]]; do
  case "$1" in
  --flow)
    FLOW="$2"
    shift 2
    ;;
  --server-tag)
    SERVER_TAG="$2"
    shift 2
    ;;
  --keep-running)
    KEEP_RUNNING=1
    shift
    ;;
  --help)
    usage
    exit 0
    ;;
  *)
    echo "Unknown argument: $1" >&2
    usage >&2
    exit 1
    ;;
  esac
done

require_cmd bash
require_cmd cargo
require_cmd curl
require_cmd docker
require_cmd gzip
require_cmd python3
docker compose version >/dev/null

ZKSYNC_OS_SERVER_IMAGE="${ZKSYNC_OS_SERVER_IMAGE:-${DEFAULT_SERVER_IMAGE_REPO}:${SERVER_TAG}}"
ANVIL_IMAGE="${ANVIL_IMAGE:-${DEFAULT_ANVIL_IMAGE}}"
DEFAULT_PRIVATE_KEY="${DEFAULT_PRIVATE_KEY:-0x7726827caac94a7f9e1b160f7ea819f172f7b6f9d2a97f992c38edeab82d4110}"

trap cleanup EXIT

prepare_workspace
write_compose_env
materialize_assets
assert_runtime_files
relax_runtime_permissions
start_stack
run_tests
