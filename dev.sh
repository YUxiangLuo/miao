#!/usr/bin/env bash
# Development entry point; installation and removal remain separate commands.
set -euo pipefail
ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage: ./dev.sh <command> [arguments]

  build [native|amd64|arm64|all] [--refresh-kernel]
      Build the panel and release binary; default native. amd64/arm64 use musl.
      Missing or stale kernel/rule assets are rebuilt automatically.
  kernel [amd64|arm64|all] [--kernel-only]
      Rebuild pinned kernels and rules; default host architecture.
  check [all|core|frontend|scripts]
      Lint, test and build checks; default all. Core needs no generated assets.
  test [--embedded] [cargo test arguments]
      Core tests, or core + CLI tests with prepared assets.
  audit
      Run frontend and Rust dependency audits (network required).
  types
      Regenerate the TypeScript API contract from Rust.
  netns [binary]
      Test a built binary with a real kernel in isolated Linux namespaces.
  version <vX.Y.Z>
      Update Cargo.toml/Cargo.lock locally; review and commit before release.
  release [--publish [--draft]]
      Default: build both musl targets and stage dist/vX.Y.Z at a clean HEAD.
      --publish: verify that staged release, push its tag and upload via gh.
  help

Build outputs: target/release/miao-rust (native),
  target/{x86_64,aarch64}-unknown-linux-musl/release/miao-rust (musl).
Cross builds require zig, cargo-zigbuild and the Rust musl targets.
EOF
}

die() { echo "dev: $*" >&2; exit 1; }
require() { local tool; for tool in "$@"; do command -v "$tool" >/dev/null || die "missing tool: $tool"; done; }
no_args() { (( $# == 0 )) || die "unexpected argument: $1 (see ./dev.sh help)"; }
host_target() {
  case "$(uname -m)" in
    x86_64) echo amd64 ;;
    aarch64) echo arm64 ;;
    *) die "unsupported host architecture: $(uname -m)" ;;
  esac
}
rust_target() {
  case "$1" in
    amd64) echo x86_64-unknown-linux-musl ;;
    arm64) echo aarch64-unknown-linux-musl ;;
    *) die "unsupported target: $1" ;;
  esac
}
frontend_build() {
  require bun
  bun install --cwd frontend-rsbuild --frozen-lockfile
  bun run --cwd frontend-rsbuild build
}
kernel_build() {
  require bun go git curl
  bash scripts/lib/kernel.sh "$@"
}
build() {
  local target=native refresh=0 specified=0 arg arch triple
  for arg in "$@"; do
    case "$arg" in
      --refresh-kernel) refresh=1 ;;
      native|amd64|arm64|all)
        (( specified == 0 )) || die "supply only one build target"
        target=$arg; specified=1 ;;
      *) die "unknown build argument: $arg" ;;
    esac
  done
  require cargo bun
  local targets=()
  case "$target" in
    native) targets=("$(host_target)") ;;
    all) targets=(amd64 arm64) ;;
    *) targets=("$target") ;;
  esac
  if [[ "$target" != native ]]; then
    require zig cargo-zigbuild rustup
    for arch in "${targets[@]}"; do
      triple=$(rust_target "$arch")
      rustup target list --installed | grep -qx "$triple" || die "install Rust target first: rustup target add $triple"
    done
  fi
  for arch in "${targets[@]}"; do
    if ! bun scripts/lib/release-artifacts.mjs resources "$arch"; then refresh=1; fi
  done
  frontend_build
  if (( refresh )); then
    if [[ "$target" == all ]]; then kernel_build all; else kernel_build "${targets[0]}"; fi
  fi
  if [[ "$target" == native ]]; then
    cargo build --locked --release -p miao-cli --target-dir "$ROOT_DIR/target"
  else
    for arch in "${targets[@]}"; do
      cargo zigbuild --locked --release -p miao-cli --target "$(rust_target "$arch")" --target-dir "$ROOT_DIR/target"
    done
  fi
}

created_files=''
cleanup() {
  if [[ -n "$created_files" ]]; then
    while IFS= read -r path; do rm -f -- "$path"; done < "$created_files"
    rm -f -- "$created_files"
  fi
}
trap cleanup EXIT
prepare_test_assets() {
  require bun
  if [[ -z "$created_files" ]]; then created_files=$(mktemp); fi
  bun scripts/tests/prepare-assets.mjs "$created_files"
}
core_check() {
  require cargo
  cargo fmt --all -- --check
  cargo clippy -p miao-core --locked --no-default-features --all-targets -- -D warnings
  cargo test -p miao-core --locked --no-default-features --all-targets
}
frontend_check() {
  require bun
  bun install --cwd frontend-rsbuild --frozen-lockfile
  bun run --cwd frontend-rsbuild lint
  bun run --cwd frontend-rsbuild typecheck
  bun run --cwd frontend-rsbuild test
  bun run --cwd frontend-rsbuild build
}
scripts_check() {
  require bun shellcheck jq
  shellcheck dev.sh install.sh remove.sh scripts/lib/*.sh scripts/tests/*.sh
  bun test scripts/tests
}

# shellcheck source=scripts/lib/release.sh
source "$ROOT_DIR/scripts/lib/release.sh"
command=${1:-help}
if (( $# )); then shift; fi
case "$command" in
  help|-h|--help) no_args "$@"; usage ;;
  build) build "$@" ;;
  kernel)
    if (( $# == 0 )) || [[ "$1" == --kernel-only ]]; then
      target=$(host_target)
    else
      target=$1; shift
    fi
    kernel_build "$target" "$@" ;;
  check)
    scope=${1:-all}
    if (( $# )); then shift; fi
    no_args "$@"
    case "$scope" in
      core) core_check ;;
      frontend) frontend_check ;;
      scripts) scripts_check ;;
      all)
        scripts_check
        frontend_check
        core_check
        prepare_test_assets
        cargo clippy --workspace --locked --all-targets --all-features -- -D warnings
        cargo test --workspace --locked --all-targets ;;
      *) die "unknown check scope: $scope" ;;
    esac ;;
  test)
    require cargo
    if [[ "${1:-}" == --embedded ]]; then
      shift
      frontend_build
      prepare_test_assets
      cargo test --workspace --locked --all-targets "$@"
    else
      cargo test -p miao-core --locked --no-default-features --all-targets "$@"
    fi ;;
  audit)
    no_args "$@"; require bun cargo-audit
    audit_status=0
    bash scripts/lib/audit-frontend.sh || audit_status=1
    cargo audit || audit_status=1
    exit "$audit_status" ;;
  types)
    no_args "$@"; require cargo
    cargo test -p miao-core --locked --no-default-features --lib \
      models::typescript_contract::export_typescript_api_bindings -- --ignored --exact ;;
  netns) exec bash scripts/tests/netns.sh "$@" ;;
  version) set_version "$@" ;;
  release) release "$@" ;;
  *) die "unknown command: $command (see ./dev.sh help)" ;;
esac
