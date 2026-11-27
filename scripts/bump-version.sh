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

# CHANGELOG.md: move the `Unreleased` entries under a dated heading for this
# version and repoint the compare links, so release notes never depend on
# someone remembering to do it by hand.
bump_changelog() {
  local file="$1" line out="" in_unreleased=false has_entries=false prev="" base today
  today="$(date -u +%F)"
  grep -qx '## \[Unreleased\]' "$file" || { echo "no [Unreleased] heading in $file" >&2; exit 1; }
  ! grep -q "^## \[$version\]" "$file" || { echo "$file already has $version" >&2; exit 1; }
  prev="$(sed -n 's|^\[Unreleased\]: .*/compare/v\(.*\)\.\.\.HEAD$|\1|p' "$file")"
  [[ -n "$prev" ]] || { echo "no [Unreleased] compare link in $file" >&2; exit 1; }
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == '## [Unreleased]' ]]; then
      in_unreleased=true
    elif [[ "$line" == '## ['* ]]; then
      in_unreleased=false
    elif [[ $in_unreleased == true && -n "$line" ]]; then
      has_entries=true
    fi
  done < "$file"
  [[ $has_entries == true ]] || { echo "$file has nothing under [Unreleased]; add entries before releasing" >&2; exit 1; }
  while IFS= read -r line || [[ -n "$line" ]]; do
    case "$line" in
      '## [Unreleased]')
        line+=$'\n\n'"## [$version] - $today"
        ;;
      '[Unreleased]: '*)
        base="${line#\[Unreleased\]: }"
        base="${base%%/compare/*}"
        line="[Unreleased]: $base/compare/v$version...HEAD"$'\n'"[$version]: $base/compare/v$prev...v$version"
        ;;
    esac
    out+="$line"$'\n'
  done < "$file"
  printf '%s' "$out" > "$file"
}

bump_changelog CHANGELOG.md
bump_cargo Cargo.toml
bump_cargo web/Cargo.toml
bump_action action.yml

cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace
.github/action/test.sh
