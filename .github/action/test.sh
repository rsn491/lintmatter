#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
action_script="$repo_root/.github/action/run.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/lintmatter-action-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

mkdir -p "$test_root/bin" "$test_root/runner-temp" "$test_root/work"

cat > "$test_root/fake-installer.sh" <<'INSTALLER'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${MOCK_INSTALL_FAIL:-false}" == true ]]; then
  exit 1
fi
mkdir -p "$CARGO_HOME/bin"
cp "$FAKE_LINTMATTER" "$CARGO_HOME/bin/lintmatter"
chmod +x "$CARGO_HOME/bin/lintmatter"
INSTALLER
chmod +x "$test_root/fake-installer.sh"

cat > "$test_root/fake-lintmatter" <<'LINTMATTER'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$ACTION_ARGS_FILE"
pwd -P > "$ACTION_PWD_FILE"
exit "${ACTION_EXIT_CODE:-0}"
LINTMATTER
chmod +x "$test_root/fake-lintmatter"

cat > "$test_root/bin/curl" <<'CURL'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${MOCK_DOWNLOAD_FAIL:-false}" == true ]]; then
  exit 22
fi
output=""
url=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      output="$2"
      shift 2
      ;;
    --proto)
      shift 2
      ;;
    --tlsv1.2 | --location | --fail | --silent | --show-error)
      shift
      ;;
    *)
      url="$1"
      shift
      ;;
  esac
done
cp "$FAKE_INSTALLER" "$output"
printf '%s\n' "$url" > "$DOWNLOAD_LOG"
CURL
chmod +x "$test_root/bin/curl"

run_action() {
  env \
    PATH="$test_root/bin:$PATH" \
    FAKE_INSTALLER="$test_root/fake-installer.sh" \
    FAKE_LINTMATTER="$test_root/fake-lintmatter" \
    ACTION_ARGS_FILE="$test_root/args" \
    ACTION_PWD_FILE="$test_root/pwd" \
    DOWNLOAD_LOG="$test_root/download" \
    LINTMATTER_RUNNER_OS=Linux \
    LINTMATTER_RUNNER_ARCH=X64 \
    LINTMATTER_RUNNER_TEMP="$test_root/runner-temp" \
    LINTMATTER_WORKING_DIRECTORY="$test_root/work" \
    LINTMATTER_VERSION=0.1.0 \
    LINTMATTER_PATHS=. \
    LINTMATTER_STRICT=false \
    LINTMATTER_QUIET=false \
    LINTMATTER_CONFIG= \
    LINTMATTER_NO_CONFIG=false \
    "$@" \
    "$action_script"
}

expect_failure() {
  local expected="$1"
  shift
  local output status
  set +e
  output="$(run_action "$@" 2>&1)"
  status=$?
  set -e
  if [[ $status -eq 0 || "$output" != *"$expected"* ]]; then
    echo "expected failure containing: $expected" >&2
    echo "status: $status" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_action \
  LINTMATTER_STRICT=true \
  LINTMATTER_QUIET=true \
  LINTMATTER_CONFIG=config/settings.yaml \
  LINTMATTER_PATHS=$'first path\nskills/second\n-leading-dash\n'

expected_args=$'--strict\n--quiet\n--config\nconfig/settings.yaml\n--\nfirst path\nskills/second\n-leading-dash'
actual_args="$(< "$test_root/args")"
[[ "$actual_args" == "$expected_args" ]] || {
  echo "action arguments were not preserved" >&2
  diff -u <(printf '%s\n' "$expected_args") "$test_root/args" >&2 || true
  exit 1
}

expected_url=https://github.com/rsn491/lintmatter/releases/download/v0.1.0/lintmatter-installer.sh
[[ "$(< "$test_root/download")" == "$expected_url" ]] || {
  echo "unexpected installer URL" >&2
  exit 1
}
expected_workdir="$(cd "$test_root/work" && pwd -P)"
[[ "$(< "$test_root/pwd")" == "$expected_workdir" ]] || {
  echo "action did not use working-directory" >&2
  echo "expected: $expected_workdir" >&2
  echo "actual: $(< "$test_root/pwd")" >&2
  exit 1
}

run_action LINTMATTER_NO_CONFIG=true
grep -Fx -- '--no-config' "$test_root/args" > /dev/null
run_action LINTMATTER_RUNNER_ARCH=ARM64

for name in LINTMATTER_STRICT LINTMATTER_QUIET LINTMATTER_NO_CONFIG; do
  expect_failure "must be 'true' or 'false'" "$name=yes"
done

expect_failure "config and no-config cannot be used together" \
  LINTMATTER_CONFIG=.lintmatter.yaml LINTMATTER_NO_CONFIG=true
expect_failure "only supports Linux runners" LINTMATTER_RUNNER_OS=macOS
expect_failure "only supports Linux runners" LINTMATTER_RUNNER_OS=Windows
expect_failure "only X64 and ARM64" LINTMATTER_RUNNER_ARCH=ARM
expect_failure "failed to download lintmatter 0.1.0" MOCK_DOWNLOAD_FAIL=true
expect_failure "installer failed" MOCK_INSTALL_FAIL=true
expect_failure "paths must not contain blank lines" LINTMATTER_PATHS=$'one\n\ntwo'

for version in latest v0.1.0 1.2 01.2.3 1.2.3-01; do
  expect_failure "version must be an exact SemVer" "LINTMATTER_VERSION=$version"
done

set +e
run_action ACTION_EXIT_CODE=17 > /dev/null 2>&1
status=$?
set -e
[[ $status -eq 17 ]] || {
  echo "lintmatter exit status was not propagated" >&2
  exit 1
}

ruby -e 'require "yaml"; YAML.load_file(ARGV.fetch(0))' "$repo_root/action.yml"
action_version="$(ruby -e 'require "yaml"; puts YAML.load_file(ARGV.fetch(0)).fetch("inputs").fetch("version").fetch("default")' "$repo_root/action.yml")"
package_version="$(cargo metadata --no-deps --format-version 1 | ruby -rjson -e 'puts JSON.parse(STDIN.read).fetch("packages").find { |p| p.fetch("name") == "lintmatter" }.fetch("version")')"
[[ "$action_version" == "$package_version" ]] || {
  echo "action.yml version $action_version does not match Cargo package version $package_version" >&2
  exit 1
}

echo "action shell tests passed"
