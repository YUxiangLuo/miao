# shellcheck shell=sh
# PASSWORD / OBFS_PASSWORD 由调用方经 stdin 注入；common.sh 已检查系统和依赖。
# 安装 Hysteria2:钉版 + 官方 release 校验和验证,替代 curl|bash 第三方
# 安装脚本(不在远端执行下载的脚本,部署结果可复现)。升级方式:
# 人工核对 changelog 后 bump HYSTERIA_VERSION。
HYSTERIA_VERSION="v2.12.1"
HYSTERIA_ASSET="hysteria-linux-${HYSTERIA_ARCH}"
HYSTERIA_BASE_URL="https://github.com/apernet/hysteria/releases/download/app/${HYSTERIA_VERSION}"

HYSTERIA_TMP="$(mktemp)"
HYSTERIA_HASHES="$(mktemp)"
HYSTERIA_STAGE="$(mktemp -d)"
trap 'rm -rf "$HYSTERIA_TMP" "$HYSTERIA_HASHES" "$HYSTERIA_STAGE"' EXIT
curl --connect-timeout 15 --max-time 120 --retry 2 -fsSLo "$HYSTERIA_TMP" "${HYSTERIA_BASE_URL}/${HYSTERIA_ASSET}" || harbor_fail "下载 Hysteria2 失败，请检查 VPS 到 GitHub 的网络和 CA 证书。"
curl --connect-timeout 15 --max-time 30 --retry 2 -fsSLo "$HYSTERIA_HASHES" "${HYSTERIA_BASE_URL}/hashes.txt" || harbor_fail "下载 Hysteria2 校验清单失败，请检查 VPS 到 GitHub 的网络。"
EXPECTED_SUM="$(awk -v f="build/${HYSTERIA_ASSET}" '$2 == f {print $1}' "$HYSTERIA_HASHES")"
if [ -z "$EXPECTED_SUM" ]; then
  harbor_fail "校验清单缺少 ${HYSTERIA_ASSET}，未替换已有服务。"
fi
ACTUAL_SUM="$(sha256sum "$HYSTERIA_TMP" | awk '{print $1}')"
if [ "$ACTUAL_SUM" != "$EXPECTED_SUM" ]; then
  harbor_fail "Hysteria2 binary checksum mismatch，下载文件校验失败，未替换已有服务。"
fi
# Prepare every generated artifact before stopping or replacing the old deployment.
install -d -m 700 "$HYSTERIA_STAGE/hysteria"
openssl req -x509 -nodes -newkey rsa:2048 -sha256 -days 3650 \
  -keyout "$HYSTERIA_STAGE/hysteria/server.key" \
  -out "$HYSTERIA_STAGE/hysteria/server.crt" \
  -subj "/CN=harbor-hysteria" >/dev/null 2>&1 || harbor_fail "生成 Hysteria2 证书失败，请检查 OpenSSL 和磁盘可写空间。"
chmod 600 "$HYSTERIA_STAGE/hysteria/server.key"
chmod 644 "$HYSTERIA_STAGE/hysteria/server.crt"

cat > "$HYSTERIA_STAGE/hysteria/config.yaml" <<EOF
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
chmod 600 "$HYSTERIA_STAGE/hysteria/config.yaml"

if [ "$HARBOR_INIT" = systemd ]; then
cat > "$HYSTERIA_STAGE/service" <<'UNIT'
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
else
cat > "$HYSTERIA_STAGE/service" <<'OPENRC'
#!/sbin/openrc-run
name="Hysteria Server"
description="Harbor Hysteria2 proxy server"
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
fi
chmod 755 "$HYSTERIA_STAGE/service"
install -m 755 "$HYSTERIA_TMP" "$HYSTERIA_STAGE/hysteria-bin"

if harbor_is_running; then WAS_RUNNING=1; else WAS_RUNNING=0; fi
if harbor_is_enabled; then WAS_ENABLED=1; else WAS_ENABLED=0; fi

HYSTERIA_BACKUP="$(mktemp -d /tmp/harbor-hysteria-backup.XXXXXX)" || harbor_fail "无法创建旧部署备份，未替换已有服务。"
backup_one() {
  src="$1" name="$2"
  if [ -e "$src" ]; then cp -a "$src" "$HYSTERIA_BACKUP/$name" || return 1; fi
}
if ! backup_one /etc/hysteria config || \
   ! backup_one /usr/local/bin/hysteria binary || \
   ! backup_one /etc/systemd/system/hysteria-server.service systemd-service || \
   ! backup_one /etc/init.d/hysteria-server openrc-service; then
  rm -rf "$HYSTERIA_BACKUP"
  harbor_fail "备份旧部署失败，未替换已有服务。"
fi

restore_old_deployment() {
  harbor_stop >/dev/null 2>&1 || true
  if harbor_is_running; then return 1; fi
  rm -rf /etc/hysteria || return 1
  rm -f /usr/local/bin/hysteria /etc/systemd/system/hysteria-server.service /etc/init.d/hysteria-server || return 1
  [ ! -e "$HYSTERIA_BACKUP/config" ] || cp -a "$HYSTERIA_BACKUP/config" /etc/hysteria || return 1
  [ ! -e "$HYSTERIA_BACKUP/binary" ] || cp -a "$HYSTERIA_BACKUP/binary" /usr/local/bin/hysteria || return 1
  [ ! -e "$HYSTERIA_BACKUP/systemd-service" ] || cp -a "$HYSTERIA_BACKUP/systemd-service" /etc/systemd/system/hysteria-server.service || return 1
  [ ! -e "$HYSTERIA_BACKUP/openrc-service" ] || cp -a "$HYSTERIA_BACKUP/openrc-service" /etc/init.d/hysteria-server || return 1
  harbor_reload_init >/dev/null 2>&1 || return 1
  restore_service_state
}

harbor_enable() {
  if [ "$HARBOR_INIT" = systemd ]; then systemctl enable "$SERVICE"; else rc-update add "$SERVICE" default; fi
}
harbor_restart() {
  if [ "$HARBOR_INIT" = systemd ]; then systemctl restart "$SERVICE" && sleep 1 && systemctl is-active --quiet "$SERVICE"
  else rc-service "$SERVICE" restart && sleep 1 && rc-service "$SERVICE" status; fi
}

# A partial old deployment may have no init file. In that case service-manager
# disable/stop commonly returns non-zero even though the requested state is
# already true. Verify the resulting state instead of treating that as a
# failed rollback.
restore_service_state() {
  # Autostart and current availability are independent. An autostart failure
  # must not prevent bringing the restored old deployment back online.
  restore_failed=0
  if [ "$WAS_ENABLED" -eq 1 ]; then
    harbor_enable || restore_failed=1
  else
    harbor_disable >/dev/null 2>&1 || true
    if harbor_is_enabled; then restore_failed=1; fi
  fi
  if [ "$WAS_RUNNING" -eq 1 ]; then
    harbor_restart || restore_failed=1
  else
    harbor_stop >/dev/null 2>&1 || true
    if harbor_is_running; then restore_failed=1; fi
  fi
  [ "$restore_failed" -eq 0 ]
}

# From this point onward every failure restores config, binary, init file and service state.
activate_new_deployment() {
  harbor_stop >/dev/null 2>&1 || true
  if harbor_is_running; then return 1; fi
  rm -rf /etc/hysteria || return 1
  rm -f /etc/systemd/system/hysteria-server.service /etc/init.d/hysteria-server || return 1
  install -d /usr/local/bin || return 1
  mv "$HYSTERIA_STAGE/hysteria" /etc/hysteria || return 1
  install -m 755 "$HYSTERIA_STAGE/hysteria-bin" /usr/local/bin/hysteria || return 1
  if [ "$HARBOR_INIT" = systemd ]; then
    install -m 644 "$HYSTERIA_STAGE/service" /etc/systemd/system/hysteria-server.service || return 1
  else
    install -m 755 "$HYSTERIA_STAGE/service" /etc/init.d/hysteria-server || return 1
  fi
  harbor_reload_init || return 1
  # A successful deploy must provide a usable node. Preserve the old service
  # state only on rollback, not when installing the requested new deployment.
  harbor_enable || return 1
  harbor_restart || return 1
}
if ! activate_new_deployment; then
  if restore_old_deployment; then
    rm -rf "$HYSTERIA_BACKUP"
    harbor_fail "Hysteria2 新部署启动失败，已恢复旧部署及服务状态。"
  else
    harbor_fail "Hysteria2 新部署失败且恢复旧部署失败；备份保留在 ${HYSTERIA_BACKUP}，请立即手动恢复。"
  fi
fi
rm -rf "$HYSTERIA_BACKUP"
