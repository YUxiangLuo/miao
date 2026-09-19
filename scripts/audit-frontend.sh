#!/usr/bin/env bash
set -euo pipefail

# Retry only temporary registry responses. Vulnerabilities and unknown failures
# remain fatal, and an unavailable registry must never count as a clean audit.
log=$(mktemp)
trap 'rm -f "$log"' EXIT
for attempt in 1 2 3; do
  status=0
  bun audit --cwd frontend-rsbuild >"$log" 2>&1 || status=$?
  cat "$log"
  if (( status == 0 )); then
    exit 0
  fi
  if (( attempt == 3 )) || ! grep -Eq '^error: POST https://registry\.npmjs\.org/-/npm/v1/security/advisories/bulk - (429|500|502|503|504)[[:space:]]*$' "$log"; then
    exit "$status"
  fi
  delay=$((attempt * 15))
  echo "Audit registry temporarily unavailable; retrying in ${delay}s (${attempt}/3)." >&2
  sleep "$delay"
done
