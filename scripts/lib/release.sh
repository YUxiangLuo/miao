# shellcheck shell=bash
# Sourced by dev.sh. Release builds and GitHub Release uploads run in GitHub Actions
# (.github/workflows/release.yml); local helpers only bump versions and safely tag/push.
clean_tree() {
  local changes
  changes=$(git status --porcelain --untracked-files=normal) || die "failed to inspect working tree"
  [[ -z "$changes" ]] || die "commit or stash changes before tagging or pushing a release"
}
set_version() (
  (( $# == 1 )) || die "usage: ./dev.sh version vX.Y.Z"
  require bun cargo
  local version=${1#v}
  [[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die "expected version vX.Y.Z"
  local backup result=0
  backup=$(mktemp -d)
  cp Cargo.toml Cargo.lock "$backup/"
  trap 'result=$?; if (( result != 0 )); then cp "$backup/Cargo.toml" "$backup/Cargo.lock" .; fi; rm -rf -- "$backup"; exit "$result"' EXIT
  bun scripts/lib/release-artifacts.mjs version "$version"
  # Resolve only the workspace version change, retaining existing dependency pins.
  cargo check -p harbor-core --offline --no-default-features
  echo "==> Version updated locally to v$version."
  echo "    Review and commit Cargo.toml/Cargo.lock, then run:"
  echo "      ./dev.sh release --push"
  echo "    or trigger the Release workflow manually in GitHub Actions."
)
release() {
  local push=0 arg version tag commit push_url remote_tag
  for arg in "$@"; do
    case "$arg" in
      --push) push=1 ;;
      *) die "unknown release argument: $arg (use ./dev.sh release [--push])" ;;
    esac
  done
  require bun git
  clean_tree
  version=$(bun scripts/lib/release-artifacts.mjs version)
  tag="v$version"
  commit=$(git rev-parse HEAD)
  if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    [[ $(git rev-parse "refs/tags/$tag^{commit}") == "$commit" ]] || die "$tag already points at another commit"
  fi
  if (( ! push )); then
    echo "==> Ready to release $tag at $commit via GitHub Actions."
    echo "    Trigger automatically by tagging and pushing:"
    echo "      ./dev.sh release --push"
    echo "    Or push your branch and run Actions -> Release -> Run workflow (supports draft mode)."
    return
  fi
  push_url=$(git remote get-url --push --all origin)
  [[ "$push_url" != *$'\n'* ]] || die "origin must have exactly one push URL"
  case "$push_url" in
    https://github.com/*|git@github.com:*|ssh://git@github.com/*) ;;
    *) die "origin must be a GitHub repository" ;;
  esac
  # Verify an existing remote tag before creating or pushing. Peel annotated tags too.
  remote_tag=$(git ls-remote --tags "$push_url" "refs/tags/$tag" "refs/tags/$tag^{}")
  if [[ -n "$remote_tag" ]]; then
    remote_tag=$(awk 'END {print $1}' <<< "$remote_tag")
    [[ "$remote_tag" == "$commit" ]] || die "remote $tag points at another commit"
  fi
  git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null || git tag "$tag" "$commit"
  git push "$push_url" HEAD "refs/tags/$tag"
  echo "==> Pushed $tag ($commit). GitHub Actions Release workflow will build and publish Linux + Windows assets."
}
