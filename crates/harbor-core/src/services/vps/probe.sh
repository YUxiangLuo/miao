# shellcheck shell=sh
# common.sh has checked root, init system and prerequisites.
CONFIG="/etc/hysteria/config.yaml"
if [ ! -f "$CONFIG" ]; then
  exit 10
fi

# 仅当证书是 harbor 部署时生成的(CN=harbor-hysteria)才复用配置,否则视为
# 第三方部署,需要清理后重新部署。
if [ ! -f /etc/hysteria/server.crt ]; then
  echo "Existing Hysteria2 service has no /etc/hysteria/server.crt; it is not deployed by Harbor and will be cleaned up" >&2
  exit 30
fi

if ! openssl x509 -in /etc/hysteria/server.crt -noout -subject 2>/dev/null | grep -Eq 'CN[[:space:]]*=[[:space:]]*harbor-hysteria'; then
  SUBJECT="$(openssl x509 -in /etc/hysteria/server.crt -noout -subject 2>/dev/null || true)"
  echo "Existing Hysteria2 service cert ($SUBJECT) is not signed for harbor-hysteria; it is not deployed by Harbor and will be cleaned up" >&2
  exit 30
fi

if ! awk '
  /^[[:space:]]*listen:[[:space:]]*:543([[:space:]]|$)/ { found = 1 }
  END { exit found ? 0 : 1 }
' "$CONFIG"; then
  echo "Existing Hysteria2 config does not listen on :543" >&2
  exit 12
fi

PASSWORD="$(awk '
  /^[^[:space:]][^:]*:/ {
    top = $1
    sub(/:$/, "", top)
  }
  top == "auth" && /^[[:space:]]*password:[[:space:]]*/ {
    sub(/^[[:space:]]*password:[[:space:]]*/, "", $0)
    gsub(/^[\"\047]|[\"\047]$/, "", $0)
    print
    found = 1
    exit
  }
  END { if (!found) exit 1 }
' "$CONFIG")"

OBFS_TYPE="$(awk '
  /^[^[:space:]][^:]*:/ {
    top = $1
    sub(/:$/, "", top)
  }
  top == "obfs" && /^[[:space:]]*type:[[:space:]]*/ {
    sub(/^[[:space:]]*type:[[:space:]]*/, "", $0)
    gsub(/^[\"\047]|[\"\047]$/, "", $0)
    print
    found = 1
    exit
  }
  END { if (!found) exit 1 }
' "$CONFIG" || true)"

GECKO_PASSWORD="$(awk '
  /^[^[:space:]][^:]*:/ {
    top = $1
    sub(/:$/, "", top)
    if (top != "obfs") in_gecko = 0
  }
  top == "obfs" && /^[[:space:]]*gecko:[[:space:]]*$/ {
    in_gecko = 1
    next
  }
  top == "obfs" && in_gecko && /^[[:space:]]*password:[[:space:]]*/ {
    sub(/^[[:space:]]*password:[[:space:]]*/, "", $0)
    gsub(/^[\"\047]|[\"\047]$/, "", $0)
    print
    found = 1
    exit
  }
  END { if (!found) exit 1 }
' "$CONFIG" || true)"

if [ -z "$PASSWORD" ]; then
  echo "Existing Hysteria2 config has no password" >&2
  exit 13
fi

if [ "$OBFS_TYPE" != "gecko" ] || [ -z "$GECKO_PASSWORD" ]; then
  if [ ! -f /etc/hysteria/server.crt ] || [ ! -f /etc/hysteria/server.key ]; then
    echo "Existing Hysteria2 config cannot be upgraded to Gecko obfs without default cert files" >&2
    exit 14
  fi

  GECKO_PASSWORD="$FALLBACK_OBFS_PASSWORD"
  # Let provision.sh upgrade both binary and config in its rollback transaction.
  # The probe must not destroy the legacy config before that backup exists.
  printf '%s\n' "$PASSWORD"
  printf '%s\n' "$GECKO_PASSWORD"
  exit 20
fi

harbor_start_checked
printf '%s\n' "$PASSWORD"
printf '%s\n' "$GECKO_PASSWORD"
