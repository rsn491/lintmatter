#!/usr/bin/env bash
# Bump every place that must carry the release version, so the lockstep check
# in .github/action/test.sh doesn't fail after a release. Pure bash so it
# behaves the same on macOS and Linux.
set -euo pipefail

version="${1:?usage: scripts/bump-version.sh X.Y.Z}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must be X.Y.Z" >&2; exit 1; }

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# Cargo manifests: the first `version = "..."` line is the package version.
bump_cargo() {
  local file="$1" line out="" done=false
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ $done == false && "$line" == 'version = "'* ]]; then
      line="version = \"$version\""
      done=true
    fi
    out+="$line"$'\n'
  done < "$file"
  [[ $done == true ]] || { echo "no version found in $file" >&2; exit 1; }
  printf '%s' "$out" > "$file"
}

# action.yml: the `default:` line of the `version` input.
bump_action() {
  local file="$1" line out="" in_version=false done=false
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == '  version:' ]]; then
      in_version=true
    elif [[ $in_version == true && $done == false && "$line" == *'default: '* ]]; then
      line="    default: $version"
      done=true
    fi
    out+="$line"$'\n'
  done < "$file"
  [[ $done == true ]] || { echo "no version default found in $file" >&2; exit 1; }
  printf '%s' "$out" > "$file"
}

bump_cargo Cargo.toml
bump_cargo web/Cargo.toml
bump_action action.yml

cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace
.github/action/test.sh
