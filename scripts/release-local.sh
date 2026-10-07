#!/usr/bin/env bash
# 本地发布：构建 Linux amd64/arm64 musl 产物并上传到 GitHub Release。
# 替代已停用的 .github/workflows/build-release.yml；Windows 版本已取消。
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

STAGE="$ROOT_DIR/dist"
TARGETS=(amd64 arm64)

dry_run=0
skip_build=0
allow_dirty=0
draft_args=()
repo_args=()
tag=""

usage() {
  cat <<'EOF'
Usage: ./scripts/release-local.sh [options] <tag>

Builds the Linux amd64/arm64 musl release assets and uploads them to the
GitHub Release for <tag>. The tag must match Cargo.toml, exist locally and
point at HEAD (run ./release.sh vX.Y.Z first).

Options:
  --dry-run          Build and stage assets in dist/ without uploading
  --skip-build       Reuse the assets already staged in dist/
  --allow-dirty      Skip the clean-tree and HEAD-at-tag checks
  --draft            Create the GitHub release as a draft
  --repo OWNER/REPO  Target repository (default: current git remote)
  -h, --help         Show this help

Assets staged in dist/:
  miao-rust-linux-amd64, miao-rust-linux-amd64.sha256
  miao-rust-linux-arm64, miao-rust-linux-arm64.sha256
  miao-embedded-sources.txt
  miao-kernel-amd64.json, miao-kernel-arm64.json
EOF
}

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required tool: $1" >&2
    exit 1
  }
}

resolve_ref() {
  local remote="$1" requested="$2" line sha resolved
  line="$(git ls-remote --exit-code "$remote" "$requested")" || return 1
  IFS=$'\t' read -r sha resolved <<< "$line" || return 1
  [[ "$resolved" == "$requested" ]] || return 1
  [[ "$sha" =~ ^[0-9a-f]{40}$ ]] || return 1
  printf '%s\n' "$sha"
}

while (( $# )); do
  case "$1" in
    --dry-run)
      dry_run=1
      shift
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    --allow-dirty)
      allow_dirty=1
      shift
      ;;
    --draft)
      draft_args=(--draft)
      shift
      ;;
    --repo)
      [[ $# -ge 2 ]] || {
        echo "--repo requires OWNER/REPO" >&2
        exit 1
      }
      repo_args=(--repo "$2")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      [[ -z "$tag" ]] || {
        echo "Only one tag may be supplied" >&2
        exit 1
      }
      tag="$1"
      shift
      ;;
  esac
done

[[ -n "$tag" ]] || {
  usage >&2
  exit 1
}
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "Invalid tag: $tag (expected vX.Y.Z)" >&2
  exit 1
}

cargo_version=$(sed -n '/^\[workspace\.package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[[ "$tag" == "v$cargo_version" ]] || {
  echo "Tag $tag does not match Cargo.toml version $cargo_version; run ./release.sh $tag first" >&2
  exit 1
}

git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null || {
  echo "Local tag $tag is missing; run ./release.sh $tag first" >&2
  exit 1
}

# 远端 tag 未推送时 gh 要等到构建结束后才报错；这里提前检查（dry-run 不上传，跳过）。
if (( ! dry_run )); then
  git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1 || {
    echo "Remote tag $tag is missing; push it with: git push origin $tag" >&2
    exit 1
  }
fi

if (( ! allow_dirty )); then
  [[ "$(git rev-parse "${tag}^{commit}")" == "$(git rev-parse HEAD)" ]] || {
    echo "HEAD is not at tag $tag; run 'git checkout $tag' before building the release" >&2
    exit 1
  }
  if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Working tree has uncommitted changes; commit or stash them first" >&2
    exit 1
  fi
fi

assets=(
  "$STAGE/miao-rust-linux-amd64"
  "$STAGE/miao-rust-linux-amd64.sha256"
  "$STAGE/miao-rust-linux-arm64"
  "$STAGE/miao-rust-linux-arm64.sha256"
  "$STAGE/miao-embedded-sources.txt"
  "$STAGE/miao-kernel-amd64.json"
  "$STAGE/miao-kernel-arm64.json"
)

if (( ! skip_build )); then
  for tool in bun cargo cargo-zigbuild zig sha256sum; do
    require "$tool"
  done

  echo "==> Building frontend"
  ./scripts/build-frontend.sh

  sing_box_ref="$(bun scripts/sing-box/source.mjs revision)"
  sing_geoip_ref="$(resolve_ref https://github.com/SagerNet/sing-geoip.git refs/heads/rule-set)"
  direct_rules_ref="$(resolve_ref https://github.com/Loyalsoldier/v2ray-rules-dat.git refs/heads/release)"

  rm -rf "$STAGE"
  mkdir -p "$STAGE"

  for target in "${TARGETS[@]}"; do
    case "$target" in
      amd64) rust_target=x86_64-unknown-linux-musl ;;
      arm64) rust_target=aarch64-unknown-linux-musl ;;
    esac
    echo "==> Building kernel and rules for $target"
    MIAO_TARGET="$target" \
      SING_BOX_REF="$sing_box_ref" SING_GEOIP_REF="$sing_geoip_ref" DIRECT_RULES_REF="$direct_rules_ref" \
      ./scripts/build-embedded.sh
    rustup target list --installed | grep -qx "$rust_target" \
      || rustup target add "$rust_target"
    echo "==> Building miao-rust for $target ($rust_target)"
    cargo zigbuild --locked --release --target "$rust_target"
    cp "target/$rust_target/release/miao-rust" "$STAGE/miao-rust-linux-$target"
    (cd "$STAGE" && sha256sum "miao-rust-linux-$target" > "miao-rust-linux-$target.sha256")
    cp "embedded/sing-box-$target.meta.json" "$STAGE/miao-kernel-$target.json"
  done

  {
    printf 'sing-box %s\n' "$sing_box_ref"
    printf 'kernel-version %s\n' "$(bun scripts/sing-box/source.mjs version)"
    printf 'kernel-profile %s\n' "$(bun scripts/sing-box/source.mjs profile)"
    printf 'go %s\n' "$(bun scripts/sing-box/source.mjs go_version)"
    printf 'build-tags %s\n' "$(bun scripts/sing-box/source.mjs build_tags)"
    printf 'sing-geoip %s\n' "$sing_geoip_ref"
    printf 'v2ray-rules-dat %s\n' "$direct_rules_ref"
  } > "$STAGE/miao-embedded-sources.txt"
fi

for asset in "${assets[@]}"; do
  [[ -f "$asset" ]] || {
    echo "Missing staged asset: $asset (drop --skip-build?)" >&2
    exit 1
  }
done

echo "==> Staged assets:"
ls -lh "$STAGE"

if (( dry_run )); then
  echo "==> Dry run: not uploading"
  exit 0
fi

require gh
gh auth status >/dev/null 2>&1 || {
  echo "gh is not authenticated; run 'gh auth login'" >&2
  exit 1
}

if gh release view "$tag" "${repo_args[@]}" >/dev/null 2>&1; then
  echo "==> Uploading assets to existing release $tag"
  gh release upload "$tag" "${assets[@]}" --clobber "${repo_args[@]}"
else
  echo "==> Creating release $tag"
  gh release create "$tag" --verify-tag --title "$tag" --generate-notes \
    "${draft_args[@]}" "${assets[@]}" "${repo_args[@]}"
fi

echo "==> Release ready: $(gh release view "$tag" "${repo_args[@]}" --json url -q .url)"
