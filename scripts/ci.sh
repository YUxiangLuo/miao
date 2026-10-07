#!/usr/bin/env bash
# 本地 CI：在开发机上复现 .github/workflows/quality.yml 的 Linux 检查。
# 仓库转私密后不再依赖 GitHub Actions；不编译 Windows 版本。
# 不启动 TUN、不接触生产实例；内核与前端只写入仓库内的 embedded/ 和 public/。
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

# prepare-test-assets.mjs 会为缺失资源创建 inert stub；结束时清理，避免留下假内核/空规则。
created_files=$(mktemp)
cleanup() {
  while IFS= read -r path; do rm -f "$path"; done < "$created_files"
  rm -f "$created_files"
}
trap cleanup EXIT

KERNEL_TARGETS=(amd64 arm64)

usage() {
  cat <<'EOF'
Usage: ./scripts/ci.sh [command]

Runs the local equivalent of the GitHub Actions quality workflow.

Commands:
  frontend         Audit, lint, typecheck, test, build the panel and test scripts
  kernel [target]  gofmt check and build the pinned kernel (default: amd64 arm64)
  rust             Rust fmt, clippy, tests and cargo-audit
  desktop          cargo check -p miao-desktop (Linux/WebKitGTK; not part of `all`)
  all              frontend + kernel + rust (default)
  quick            frontend + rust, reusing the existing embedded kernel
  help             Show this help

Environment:
  CI_SKIP_AUDIT=1  Skip the frontend and cargo network audits
EOF
}

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required tool: $1" >&2
    exit 1
  }
}

frontend() {
  require bun
  echo "==> Frontend: install dependencies"
  bun install --cwd frontend-rsbuild --frozen-lockfile
  if [[ "${CI_SKIP_AUDIT:-0}" != 1 ]]; then
    echo "==> Frontend: audit"
    bash scripts/audit-frontend.sh
  fi
  echo "==> Frontend: lint"
  bun run --cwd frontend-rsbuild lint
  echo "==> Frontend: typecheck"
  bun run --cwd frontend-rsbuild typecheck
  echo "==> Frontend: test"
  bun run --cwd frontend-rsbuild test
  echo "==> Scripts: test"
  bun test scripts
  echo "==> Frontend: build"
  bun run --cwd frontend-rsbuild build
}

smoke_kernel() {
  case "$1" in
    amd64)
      ./embedded/sing-box-amd64 version
      ;;
    arm64)
      echo "==> Kernel: skip arm64 smoke test (cross-compiled)"
      ;;
    *)
      echo "==> Kernel: skip $1 smoke test"
      ;;
  esac
}

kernel() {
  require go
  require bun
  local targets=("$@")
  if (( ${#targets[@]} == 0 )); then
    targets=("${KERNEL_TARGETS[@]}")
  fi
  echo "==> Kernel: gofmt check"
  local unformatted
  unformatted=$(gofmt -l scripts/sing-box/tests/*.go)
  if [[ -n "$unformatted" ]]; then
    echo "gofmt needed for:" >&2
    echo "$unformatted" >&2
    exit 1
  fi
  for target in "${targets[@]}"; do
    echo "==> Kernel: build $target"
    MIAO_TARGET="$target" ./scripts/build-embedded.sh --kernel-only
    smoke_kernel "$target"
  done
}

rust() {
  require cargo
  require bun
  bun scripts/prepare-test-assets.mjs "$created_files"
  echo "==> Rust: fmt"
  cargo fmt --all -- --check
  echo "==> Rust: clippy"
  cargo clippy --locked --all-targets --all-features -- -D warnings
  echo "==> Rust: test"
  cargo test --locked --all-targets
  if [[ "${CI_SKIP_AUDIT:-0}" != 1 ]]; then
    require cargo-audit
    echo "==> Rust: audit"
    cargo audit
  fi
}

desktop() {
  require cargo
  echo "==> Desktop: cargo check -p miao-desktop"
  cargo check -p miao-desktop --locked
}

command="${1:-all}"
case "$command" in
  frontend)
    frontend
    ;;
  kernel)
    shift
    kernel "$@"
    ;;
  rust)
    rust
    ;;
  desktop)
    desktop
    ;;
  all)
    frontend
    kernel
    rust
    ;;
  quick)
    frontend
    rust
    ;;
  help|-h|--help)
    usage
    ;;
  *)
    echo "Unknown command: $command" >&2
    usage >&2
    exit 1
    ;;
esac

echo "==> Local CI passed: $command"
