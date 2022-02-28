#!/usr/bin/env bash

[[ $DEBUG = true ]] && set -x
set -euo pipefail

readonly APP_DIR="${APP_DIR:-/app}"
readonly APP_BIN="${APP_BIN:-${APP_DIR}/massa-node}"
readonly CONFIG="${CONFIG:-${APP_DIR}/config.toml}"

start_app() {
  local args=("$@")

  echo "Starting: ${APP_BIN} ${args[*]}"
  exec "${APP_BIN}" "${args[@]}"
}

start_app ${@}
