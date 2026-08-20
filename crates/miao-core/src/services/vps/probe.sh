set -euo pipefail
# FALLBACK_OBFS_PASSWORD 由调用方以变量前缀形式经 stdin 注入(不进远端 argv)
CONFIG="/etc/hysteria/config.yaml"
SERVICE="hysteria-server.service"

if [ "$(id -u)" -ne 0 ]; then
  echo "Miao VPS config probe requires root SSH access" >&2
  exit 20
fi

if [ ! -f "$CONFIG" ]; then
  exit 10
fi

for cmd in awk grep openssl systemctl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 11
  fi
done

# 仅当证书是 miao 部署时生成的(CN=miao-hysteria)才复用配置,否则视为
# 第三方部署,需要清理后重新部署。
if [ ! -f /etc/hysteria/server.crt ]; then
  echo "Existing Hysteria2 service has no /etc/hysteria/server.crt; it is not deployed by Miao and will be cleaned up" >&2
  exit 30
fi

if ! openssl x509 -in /etc/hysteria/server.crt -noout -subject 2>/dev/null | grep -Eq 'CN[[:space:]]*=[[:space:]]*miao-hysteria'; then
  SUBJECT="$(openssl x509 -in /etc/hysteria/server.crt -noout -subject 2>/dev/null || true)"
  echo "Existing Hysteria2 service cert ($SUBJECT) is not signed for miao-hysteria; it is not deployed by Miao and will be cleaned up" >&2
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
  cat > "$CONFIG" <<EOF
listen: :543
tls:
  cert: /etc/hysteria/server.crt
  key: /etc/hysteria/server.key
auth:
  type: password
  password: ${PASSWORD}
obfs:
  type: gecko
  gecko:
    password: ${GECKO_PASSWORD}
masquerade:
  type: proxy
  proxy:
    url: https://www.bing.com/
    rewriteHost: true
EOF
  chmod 600 "$CONFIG"
fi

systemctl enable "$SERVICE" >/dev/null 2>&1 || true
systemctl restart "$SERVICE"
systemctl is-active --quiet "$SERVICE"
printf '%s\n' "$PASSWORD"
printf '%s\n' "$GECKO_PASSWORD"
