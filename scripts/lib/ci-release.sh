#!/usr/bin/env bash
# GitHub Actions release preflight and publish helper.
# Keeps all validation, tag checks, asset upload, and cleanup ShellCheck-clean and unit-tested.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

die() { echo "ci-release: $*" >&2; exit 1; }

resolve_ref() {
  local sha ref
  while IFS=$'\t' read -r sha ref; do
    if [[ "$ref" == "$2" && "$sha" =~ ^[0-9a-f]{40}$ ]]; then
      printf '%s\n' "$sha"
      return 0
    fi
  done < <(git ls-remote --exit-code "$1" "$2")
  return 1
}

check_remote_tag() {
  local tag=$1 commit=$2 remote_tag
  remote_tag=$(git ls-remote --tags origin "refs/tags/$tag" "refs/tags/$tag^{}")
  if [[ -n "$remote_tag" ]]; then
    remote_tag=$(awk 'END {print $1}' <<< "$remote_tag")
    [[ "$remote_tag" == "$commit" ]] || die "remote $tag points at $remote_tag, expected $commit"
    echo "present"
  else
    echo "absent"
  fi
}

query_release_state() {
  local repo=$1 tag=$2
  gh api --paginate "repos/$repo/releases?per_page=100" \
    --jq ".[] | select(.tag_name == \"$tag\") | (if .draft then \"draft\" else \"published\" end), .assets[].name"
}

write_output() {
  local key=$1 value=$2
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf '%s=%s\n' "$key" "$value" >> "$GITHUB_OUTPUT"
  else
    printf '%s=%s\n' "$key" "$value"
  fi
}

cmd_preflight() {
  local event_name=${EVENT_NAME:-push}
  local ref_name=${REF_NAME:-}
  local commit=${COMMIT:-}
  if [[ -z "$commit" ]]; then commit=$(git rev-parse HEAD); fi
  local expected_version=${INPUT_EXPECTED_VERSION:-}
  local draft=${INPUT_DRAFT:-false}
  local repo=${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}
  local version tag go_version sing_geoip_ref direct_rules_ref release_state state

  [[ "$commit" =~ ^[0-9a-f]{40}$ ]] || die "invalid commit sha: $commit"
  [[ "$draft" == true || "$draft" == false ]] || die "invalid draft flag: $draft"

  version=$(bun scripts/lib/release-artifacts.mjs version)
  tag="v$version"

  if [[ "$event_name" == push ]]; then
    [[ "$ref_name" == "$tag" ]] || die "pushed tag '$ref_name' does not match Cargo.toml version '$tag'"
    draft=false
  elif [[ "$event_name" == workflow_dispatch ]]; then
    if [[ -n "$expected_version" ]]; then
      local normalized="v${expected_version#v}"
      [[ "$normalized" == "$tag" ]] || die "expected_version '$expected_version' does not match Cargo.toml version '$tag'"
    fi
    if [[ "${GITHUB_REF_TYPE:-}" == tag && -n "$ref_name" ]]; then
      [[ "$ref_name" == "$tag" ]] || die "selected tag '$ref_name' does not match Cargo.toml version '$tag'"
    fi
  else
    die "unsupported event: $event_name"
  fi

  [[ "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die "expected semver tag vX.Y.Z, got $tag"

  check_remote_tag "$tag" "$commit" >/dev/null
  release_state=$(query_release_state "$repo" "$tag")
  state=${release_state%%$'\n'*}
  if [[ "$draft" == true && "$state" == published ]]; then
    die "$tag is already published; draft mode cannot replace a public release"
  fi

  go_version=$(bun scripts/sing-box/source.mjs go_version)
  sing_geoip_ref=$(resolve_ref https://github.com/SagerNet/sing-geoip.git refs/heads/rule-set)
  direct_rules_ref=$(resolve_ref https://github.com/Loyalsoldier/v2ray-rules-dat.git refs/heads/release)

  write_output tag "$tag"
  write_output version "$version"
  write_output commit "$commit"
  write_output draft "$draft"
  write_output go_version "$go_version"
  write_output sing_geoip_ref "$sing_geoip_ref"
  write_output direct_rules_ref "$direct_rules_ref"
}

cmd_publish() {
  (( $# == 4 )) || die "usage: ci-release.sh publish <stage_dir> <commit> <tag> <draft>"
  local stage=$1 commit=$2 tag=$3 draft=$4
  local repo=${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}
  local release_state state tag_presence asset_name url
  local canonical_names=() assets=()

  [[ "$draft" == true || "$draft" == false ]] || die "invalid draft flag: $draft"

  bun scripts/lib/release-artifacts.mjs assemble "$stage" "$commit" "$tag"
  bun scripts/lib/release-artifacts.mjs verify "$stage" "$commit" "$tag"

  while IFS= read -r asset_name; do
    [[ -n "$asset_name" ]] || continue
    canonical_names+=("$asset_name")
    assets+=("$stage/$asset_name")
  done < <(bun scripts/lib/release-artifacts.mjs assets)
  (( ${#assets[@]} > 0 )) || die "no canonical release assets returned"

  if command -v sha256sum >/dev/null 2>&1; then
    for asset_name in "${canonical_names[@]}"; do
      if [[ "$asset_name" == *.sha256 ]]; then
        (cd "$stage" && sha256sum -c "$asset_name")
      fi
    done
  fi

  tag_presence=$(check_remote_tag "$tag" "$commit")
  release_state=$(query_release_state "$repo" "$tag")
  state=${release_state%%$'\n'*}
  if [[ "$draft" == true && "$state" == published ]]; then
    die "$tag is already published; draft mode cannot replace a public release"
  fi

  if [[ "$tag_presence" == absent ]]; then
    git tag -f "$tag" "$commit"
    git push origin "refs/tags/$tag"
  fi

  if [[ -n "$release_state" ]]; then
    gh release upload "$tag" --repo "$repo" "${assets[@]}" --clobber
    local existing keep
    while IFS= read -r existing; do
      [[ -n "$existing" ]] || continue
      keep=0
      for asset_name in "${canonical_names[@]}"; do
        if [[ "$existing" == "$asset_name" ]]; then
          keep=1
          break
        fi
      done
      if (( ! keep )); then
        gh release delete-asset "$tag" "$existing" --repo "$repo" --yes
      fi
    done < <(printf '%s\n' "$release_state" | tail -n +2)
    if [[ "$draft" == false && "$state" == draft ]]; then
      gh release edit "$tag" --repo "$repo" --draft=false
    fi
  else
    local draft_args=()
    if [[ "$draft" == true ]]; then draft_args=(--draft); fi
    gh release create "$tag" --repo "$repo" --verify-tag --title "$tag" --generate-notes "${draft_args[@]}" "${assets[@]}"
  fi

  url=$(gh release view "$tag" --repo "$repo" --json url -q .url)
  echo "$url"
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    bun scripts/lib/release-artifacts.mjs summary "$stage" "$url" >> "$GITHUB_STEP_SUMMARY"
  fi
}

command=${1:-}
if (( $# )); then shift; fi
case "$command" in
  preflight) cmd_preflight "$@" ;;
  publish) cmd_publish "$@" ;;
  *) die "unknown command: ${command:-<empty>} (expected preflight or publish)" ;;
esac
