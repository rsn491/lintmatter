#!/usr/bin/env bash

set -euo pipefail

error() {
  echo "lintmatter action: $*" >&2
  exit 2
}

boolean() {
  local name="$1"
  local value="$2"

  case "$value" in
    true) printf '%s' true ;;
    false) printf '%s' false ;;
    *) error "$name must be 'true' or 'false' (received '$2')" ;;
  esac
}

runner_os="${LINTMATTER_RUNNER_OS:-}"
runner_arch="${LINTMATTER_RUNNER_ARCH:-}"
version="${LINTMATTER_VERSION:-}"
workdir="${LINTMATTER_WORKING_DIRECTORY:-}"
runner_temp="${LINTMATTER_RUNNER_TEMP:-}"
config="${LINTMATTER_CONFIG:-}"
strict="$(boolean strict "${LINTMATTER_STRICT:-}")"
quiet="$(boolean quiet "${LINTMATTER_QUIET:-}")"
no_config="$(boolean no-config "${LINTMATTER_NO_CONFIG:-}")"

[[ "$runner_os" == Linux ]] || error "lintmatter only supports Linux runners (received '$runner_os')"
case "$runner_arch" in
  X64 | ARM64) ;;
  *) error "lintmatter supports only X64 and ARM64 Linux runners (received '$runner_arch')" ;;
esac

# An exact SemVer keeps user input out of both URL structure and release aliases.
semver='^([0-9]+)\.([0-9]+)\.([0-9]+)(-([0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*))?(\+([0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*))?$'
[[ "$version" =~ $semver ]] || error "version must be an exact SemVer without a leading 'v' (received '$version')"
for number in "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}"; do
  [[ "$number" == 0 || "$number" != 0* ]] || error "version must be an exact SemVer without leading zeroes (received '$version')"
done
if [[ -n "${BASH_REMATCH[5]}" ]]; then
  IFS=. read -r -a prerelease_identifiers <<< "${BASH_REMATCH[5]}"
  for identifier in "${prerelease_identifiers[@]}"; do
    if [[ "$identifier" =~ ^[0-9]+$ && "$identifier" != 0 && "$identifier" == 0* ]]; then
      error "version must be an exact SemVer without leading zeroes (received '$version')"
    fi
  done
fi

[[ -n "$workdir" ]] || error "working-directory must not be empty"
[[ -d "$workdir" ]] || error "working-directory is not a directory: $workdir"
[[ -n "$runner_temp" ]] || error "runner temporary directory is unavailable"
[[ -d "$runner_temp" ]] || error "runner temporary directory is not a directory: $runner_temp"

if [[ -n "$config" && "$no_config" == true ]]; then
  error "config and no-config cannot be used together"
fi

args=()
[[ "$strict" == true ]] && args+=(--strict)
[[ "$quiet" == true ]] && args+=(--quiet)
[[ -n "$config" ]] && args+=(--config "$config")
[[ "$no_config" == true ]] && args+=(--no-config)

paths=()
while IFS= read -r path || [[ -n "$path" ]]; do
  path="${path%$'\r'}"
  [[ -n "$path" ]] || error "paths must not contain blank lines"
  paths+=("$path")
done < <(printf '%s' "${LINTMATTER_PATHS:-}")
[[ ${#paths[@]} -gt 0 ]] || error "paths must contain at least one file or directory"
args+=(-- "${paths[@]}")

install_root="$(mktemp -d "$runner_temp/lintmatter-action.XXXXXX")"
installer="$install_root/lintmatter-installer.sh"
installer_url="https://github.com/rsn491/lintmatter/releases/download/v${version}/lintmatter-installer.sh"

if ! curl --proto '=https' --tlsv1.2 --location --fail --silent --show-error \
  --output "$installer" "$installer_url"; then
  error "failed to download lintmatter $version from $installer_url"
fi

if ! CARGO_HOME="$install_root" \
  XDG_CONFIG_HOME="$install_root/config" \
  LINTMATTER_NO_MODIFY_PATH=1 \
  bash "$installer"; then
  error "the lintmatter $version installer failed"
fi

binary="$install_root/bin/lintmatter"
[[ -x "$binary" ]] || error "the lintmatter $version installer did not produce an executable"

cd "$workdir"
exec "$binary" "${args[@]}"
