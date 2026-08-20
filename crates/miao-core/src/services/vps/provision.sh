set -euo pipefail
# PASSWORD / OBFS_PASSWORD 由调用方以变量前缀形式经 stdin 注入(不进远端 argv)
SERVICE="hysteria-server.service"

if [ "$(id -u)" -ne 0 ]; then
  echo "Miao VPS provisioning requires root SSH access" >&2
  exit 1
fi

for cmd in bash curl systemctl openssl sha256sum awk mktemp; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 1
  fi
done

# 重新部署前先清理远程可能存在的非 Miao 部署的 hysteria 残留,
# 保证从干净状态开始(探针复用失败时也会经过这里)。
systemctl stop "$SERVICE" >/dev/null 2>&1 || true
systemctl disable "$SERVICE" >/dev/null 2>&1 || true
pkill -x hysteria >/dev/null 2>&1 || true
rm -rf /etc/hysteria
rm -f /usr/local/bin/hysteria

# 安装 Hysteria2:钉版 + 官方 release 校验和验证,替代 curl|bash 第三方
# 安装脚本(不在远端执行下载的脚本,部署结果可复现)。升级方式:
# 人工核对 changelog 后 bump HYSTERIA_VERSION。
HYSTERIA_VERSION="v2.12.1"
case "$(uname -m)" in
  x86_64) HYSTERIA_ARCH="amd64" ;;
  aarch64|arm64) HYSTERIA_ARCH="arm64" ;;
  *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
HYSTERIA_ASSET="hysteria-linux-${HYSTERIA_ARCH}"
HYSTERIA_BASE_URL="https://github.com/apernet/hysteria/releases/download/app/${HYSTERIA_VERSION}"

HYSTERIA_TMP="$(mktemp)"
trap 'rm -f "$HYSTERIA_TMP"' EXIT
curl -fsSLo "$HYSTERIA_TMP" "${HYSTERIA_BASE_URL}/${HYSTERIA_ASSET}"
EXPECTED_SUM="$(curl -fsSL "${HYSTERIA_BASE_URL}/hashes.txt" | awk -v f="build/${HYSTERIA_ASSET}" '$2 == f {print $1}')"
if [ -z "$EXPECTED_SUM" ]; then
  echo "Failed to resolve checksum for ${HYSTERIA_ASSET}" >&2
  exit 1
fi
ACTUAL_SUM="$(sha256sum "$HYSTERIA_TMP" | awk '{print $1}')"
if [ "$ACTUAL_SUM" != "$EXPECTED_SUM" ]; then
  echo "Hysteria2 binary checksum mismatch" >&2
  exit 1
fi
install -m 755 "$HYSTERIA_TMP" /usr/local/bin/hysteria
rm -f "$HYSTERIA_TMP"
trap - EXIT

install -d -m 700 /etc/hysteria
openssl req -x509 -nodes -newkey rsa:2048 -sha256 -days 3650 \
  -keyout /etc/hysteria/server.key \
  -out /etc/hysteria/server.crt \
  -subj "/CN=miao-hysteria" >/dev/null 2>&1
chmod 600 /etc/hysteria/server.key
chmod 644 /etc/hysteria/server.crt

cat > /etc/hysteria/config.yaml <<EOF
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
    password: ${OBFS_PASSWORD}
masquerade:
  type: proxy
  proxy:
    url: https://www.bing.com/
    rewriteHost: true
EOF
chmod 600 /etc/hysteria/config.yaml

cat > /etc/systemd/system/hysteria-server.service <<'UNIT'
[Unit]
Description=Hysteria Server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/hysteria server -c /etc/hysteria/config.yaml
Restart=on-failure
RestartSec=5
User=root

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload

systemctl enable "$SERVICE"
systemctl restart "$SERVICE"
systemctl is-active --quiet "$SERVICE"
