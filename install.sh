#!/usr/bin/env bash
set -euo pipefail

REPO="stashbase/cli"
BINARY_NAME="stashbase"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"
LATEST_CHECKSUMS_URL="https://github.com/${REPO}/releases/latest/download/checksums.txt"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

log() {
  printf '%s\n' "$*"
}

fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

detect_downloader() {
  if need_cmd curl; then
    DOWNLOADER="curl"
  elif need_cmd wget; then
    DOWNLOADER="wget"
  else
    fail "curl or wget is required"
  fi
}

check_prerequisites() {
  need_cmd mktemp || fail "mktemp is required"
  need_cmd tar || fail "tar is required"
  need_cmd install || fail "install is required"
}

download_to_file() {
  local url="$1"
  local output="$2"

  if [ "${DOWNLOADER}" = "curl" ]; then
    curl -fsSL "${url}" -o "${output}"
  else
    wget -qO "${output}" "${url}"
  fi
}

download_to_stdout() {
  local url="$1"

  if [ "${DOWNLOADER}" = "curl" ]; then
    curl -fsSL "${url}"
  else
    wget -qO- "${url}"
  fi
}

detect_target() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "${os}-${arch}" in
    darwin-arm64|darwin-aarch64)
      TARGET="aarch64-apple-darwin"
      ;;
    linux-x86_64)
      TARGET="x86_64-unknown-linux-gnu"
      ;;
    darwin-x86_64)
      fail "Intel macOS is not supported in the beta. Use an Apple Silicon Mac or install from source."
      ;;
    *)
      fail "unsupported platform: ${os}-${arch}"
      ;;
  esac
}

choose_install_dir() {
  if [ -n "${INSTALL_DIR:-}" ]; then
    TARGET_INSTALL_DIR="${INSTALL_DIR}"
    return
  fi

  if need_cmd "${BINARY_NAME}"; then
    local existing_bin
    existing_bin="$(command -v "${BINARY_NAME}")"
    TARGET_INSTALL_DIR="$(dirname "${existing_bin}")"
    return
  fi

  if [ -d "${DEFAULT_INSTALL_DIR}" ] || mkdir -p "${DEFAULT_INSTALL_DIR}" 2>/dev/null; then
    TARGET_INSTALL_DIR="${DEFAULT_INSTALL_DIR}"
    return
  fi

  TARGET_INSTALL_DIR="/usr/local/bin"
}

verify_checksum() {
  local checksum_file="$1"
  local tarball_name="$2"
  local tarball_path="$3"
  local expected
  local actual

  expected="$(sed -n "s/^\([0-9a-fA-F][0-9a-fA-F]*\)  ${tarball_name}\$/\1/p" "${checksum_file}" | head -n 1)"
  [ -n "${expected}" ] || fail "checksum for ${tarball_name} not found in checksums.txt"

  if need_cmd sha256sum; then
    actual="$(sha256sum "${tarball_path}" | awk '{print $1}')"
  elif need_cmd shasum; then
    actual="$(shasum -a 256 "${tarball_path}" | awk '{print $1}')"
  elif need_cmd openssl; then
    actual="$(openssl dgst -sha256 "${tarball_path}" | awk '{print $NF}')"
  else
    fail "sha256sum, shasum, or openssl is required to verify downloads"
  fi

  [ "${expected}" = "${actual}" ] || fail "checksum verification failed for ${tarball_name}"
}

extract_version_from_tarball_name() {
  local tarball_name="$1"
  local prefix="${BINARY_NAME}-"
  local suffix="-${TARGET}.tar.gz"

  case "${tarball_name}" in
    "${prefix}"*"${suffix}")
      printf '%s\n' "${tarball_name#${prefix}}" | sed "s/${suffix}\$//"
      ;;
    *)
      fail "could not extract version from ${tarball_name}"
      ;;
  esac
}

resolve_release_from_checksums() {
  local checksum_file="$1"
  local version_input="${2:-}"
  local tarball_name version

  if [ -n "${version_input}" ] && [ "${version_input}" != "latest" ]; then
    version="${version_input#v}"
    tarball_name="${BINARY_NAME}-${version}-${TARGET}.tar.gz"
    printf '%s\n%s\n' "${version}" "${tarball_name}"
    return
  fi

  tarball_name="$(
    awk '{print $2}' "${checksum_file}" | grep "^${BINARY_NAME}-.*-${TARGET}\.tar\.gz$" | head -n 1
  )"
  [ -n "${tarball_name}" ] || fail "no release artifact found for target ${TARGET} in checksums.txt"

  version="$(extract_version_from_tarball_name "${tarball_name}")"
  printf '%s\n%s\n' "${version}" "${tarball_name}"
}

validate_archive_layout() {
  local tarball_path="$1"

  if ! tar -tzf "${tarball_path}" | grep -qx "${BINARY_NAME}"; then
    fail "archive did not contain a top-level ${BINARY_NAME} binary"
  fi
}

install_binary() {
  local source_bin="$1"
  local destination="${TARGET_INSTALL_DIR}/${BINARY_NAME}"

  mkdir -p "${TARGET_INSTALL_DIR}" 2>/dev/null || true

  if [ -w "${TARGET_INSTALL_DIR}" ]; then
    install -m 0755 "${source_bin}" "${destination}"
  elif need_cmd sudo; then
    log "Administrator permission is required to install into ${TARGET_INSTALL_DIR}."
    sudo install -m 0755 "${source_bin}" "${destination}"
  else
    fail "cannot write to ${TARGET_INSTALL_DIR}; set INSTALL_DIR to a writable directory"
  fi
}

warn_if_not_on_path() {
  case ":${PATH}:" in
    *":${TARGET_INSTALL_DIR}:"*) ;;
    *)
      log "Warning: ${TARGET_INSTALL_DIR} is not on your PATH."
      log "Add this line to your shell profile:"
      log "  export PATH=\"${TARGET_INSTALL_DIR}:\$PATH\""
      ;;
  esac
}

main() {
  detect_downloader
  check_prerequisites
  detect_target
  choose_install_dir

  local version tag base_url tarball_name tarball_url tarball_path checksums_url checksums_path release_info
  checksums_url="${LATEST_CHECKSUMS_URL}"
  checksums_path="${TMP_DIR}/checksums.txt"

  download_to_file "${checksums_url}" "${checksums_path}"

  release_info="$(resolve_release_from_checksums "${checksums_path}" "${STASHBASE_VERSION:-latest}")"
  version="$(printf '%s\n' "${release_info}" | sed -n '1p')"
  tarball_name="$(printf '%s\n' "${release_info}" | sed -n '2p')"
  tag="v${version}"
  base_url="https://github.com/${REPO}/releases/download/${tag}"
  tarball_url="${base_url}/${tarball_name}"
  tarball_path="${TMP_DIR}/${tarball_name}"

  log "Installing ${BINARY_NAME} ${version} for ${TARGET}..."
  download_to_file "${tarball_url}" "${tarball_path}"
  verify_checksum "${checksums_path}" "${tarball_name}" "${tarball_path}"
  validate_archive_layout "${tarball_path}"

  tar -xzf "${tarball_path}" -C "${TMP_DIR}"
  [ -f "${TMP_DIR}/${BINARY_NAME}" ] || fail "archive did not contain ${BINARY_NAME}"

  install_binary "${TMP_DIR}/${BINARY_NAME}"

  log "Installed ${BINARY_NAME} ${version} to ${TARGET_INSTALL_DIR}/${BINARY_NAME}"
  warn_if_not_on_path
  log "Run '${BINARY_NAME} --help' to get started."
}

main "$@"
