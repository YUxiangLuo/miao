# PASSWORD / OBFS_PASSWORD 由调用方经 stdin 注入；common.sh 已检查系统和依赖。
# 安装 Hysteria2:钉版 + 官方 release 校验和验证,替代 curl|bash 第三方
# 安装脚本(不在远端执行下载的脚本,部署结果可复现)。升级方式:
# 人工核对 changelog 后 bump HYSTERIA_VERSION。
HYSTERIA_VERSION="v2.12.1"
HYSTERIA_ASSET="hysteria-linux-${HYSTERIA_ARCH}"
HYSTERIA_BASE_URL="https://github.com/apernet/hysteria/releases/download/app/${HYSTERIA_VERSION}"

HYSTERIA_TMP="$(mktemp)"
HYSTERIA_HASHES="$(mktemp)"
trap 'rm -f "$HYSTERIA_TMP" "$HYSTERIA_HASHES"' EXIT
curl --connect-timeout 15 --max-time 120 --retry 2 -fsSLo "$HYSTERIA_TMP" "${HYSTERIA_BASE_URL}/${HYSTERIA_ASSET}" || miao_fail "下载 Hysteria2 失败，请检查 VPS 到 GitHub 的网络和 CA 证书。"
curl --connect-timeout 15 --max-time 30 --retry 2 -fsSLo "$HYSTERIA_HASHES" "${HYSTERIA_BASE_URL}/hashes.txt" || miao_fail "下载 Hysteria2 校验清单失败，请检查 VPS 到 GitHub 的网络。"
EXPECTED_SUM="$(awk -v f="build/${HYSTERIA_ASSET}" '$2 == f {print $1}' "$HYSTERIA_HASHES")"
if [ -z "$EXPECTED_SUM" ]; then
  miao_fail "校验清单缺少 ${HYSTERIA_ASSET}，未替换已有服务。"
fi
ACTUAL_SUM="$(sha256sum "$HYSTERIA_TMP" | awk '{print $1}')"
if [ "$ACTUAL_SUM" != "$EXPECTED_SUM" ]; then
  miao_fail "Hysteria2 binary checksum mismatch，下载文件校验失败，未替换已有服务。"
fi
# Do not touch an existing deployment until download and verification succeed.
miao_stop >/dev/null 2>&1 || true
miao_disable >/dev/null 2>&1 || true
pkill -x hysteria >/dev/null 2>&1 || true
rm -rf /etc/hysteria
install -d /usr/local/bin
install -m 755 "$HYSTERIA_TMP" /usr/local/bin/hysteria
rm -f "$HYSTERIA_TMP" "$HYSTERIA_HASHES"
trap - EXIT

install -d -m 700 /etc/hysteria
openssl req -x509 -nodes -newkey rsa:2048 -sha256 -days 3650 \
  -keyout /etc/hysteria/server.key \
  -out /etc/hysteria/server.crt \
  -subj "/CN=miao-hysteria" >/dev/null 2>&1 || miao_fail "生成 Hysteria2 证书失败，请检查 OpenSSL 和磁盘可写空间。"
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

if [ "$MIAO_INIT" = systemd ]; then
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

else
cat > /etc/init.d/hysteria-server <<'OPENRC'
#!/sbin/openrc-run
name="Hysteria Server"
description="Miao Hysteria2 proxy server"
command="/usr/local/bin/hysteria"
command_args="server -c /etc/hysteria/config.yaml"
command_background=true
pidfile="/run/hysteria-server.pid"
output_log="/var/log/hysteria-server.log"
error_log="/var/log/hysteria-server.log"
depend() {
  need net
  after firewall
}
OPENRC
chmod 755 /etc/init.d/hysteria-server
fi
miao_start_checked
