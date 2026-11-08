#!/usr/bin/env bash
# Bump every place that must carry the release version, so the lockstep check
# in .github/action/test.sh doesn't fail after a release.
set -euo pipefail

version="${1:?usage: scripts/bump-version.sh X.Y.Z}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must be X.Y.Z" >&2; exit 1; }

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

perl -0pi -e "s/^version = \".*?\"/version = \"$version\"/m" Cargo.toml web/Cargo.toml
perl -0pi -e "s/(Exact lintmatter CLI version[^\n]*\n[^\n]*\n\s*default: )\S+/\${1}$version/" action.yml

cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace
.github/action/test.sh
