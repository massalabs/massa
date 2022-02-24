#!/usr/bin/env bash

[[ $DEBUG = true ]] && set -x
set -euo pipefail

readonly APP_DIR="${APP_DIR:-/app}"
readonly APP_BIN="${APP_BIN:-${APP_DIR}/massa-node}"
readonly CONFIG="${CONFIG:-${APP_DIR}/config.toml}"
readonly SECRETS_DIR="${SECRETS_DIR:-/etc/.secrets}"

wait_for_secrets() {
  if [[ -d ${SECRETS_DIR} ]]; then
    echo "[INFO]: waiting for ${SECRETS_DIR}/.mounted..."
    [[ -f "${SECRETS_DIR}/.mounted" ]] && return
    sleep 1s
  fi
}

start_app() {
  local args=("$@")

  re=" (--config|-c) "
  if [[ ! " ${args[@]} " =~ $re && -f "${CONFIG}" ]]; then
      args+=( --config "${CONFIG}" )
  fi

  echo "Starting: ${APP_BIN} ${args[*]}"
  exec "${APP_BIN}" "${args[@]}"
}

#wait_for_secrets
start_app ${@}
