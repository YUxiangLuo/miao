# shellcheck shell=sh
# Shared POSIX shell preflight; works on minimal VPS images without bash.
set -eu
export LC_ALL=C
SERVICE="hysteria-server"

harbor_fail() {
  echo "HARBOR_ERROR: $*" >&2
  exit 1
}

[ "$(id -u)" -eq 0 ] || harbor_fail "需要 root 权限，请使用 root SSH 登录。"
[ "$(uname -s)" = Linux ] || harbor_fail "仅支持 Linux VPS。"
# Used by provision.sh, which is appended to this preflight.
export HYSTERIA_ARCH
case "$(uname -m)" in
  x86_64) HYSTERIA_ARCH="amd64" ;;
  aarch64|arm64) HYSTERIA_ARCH="arm64" ;;
  *) harbor_fail "不支持的 CPU 架构：$(uname -m)。仅支持 x86_64 / arm64。" ;;
esac

if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
  HARBOR_INIT=systemd
elif command -v rc-service >/dev/null 2>&1 && command -v rc-update >/dev/null 2>&1; then
  HARBOR_INIT=openrc
else
  harbor_fail "未检测到运行中的 systemd 或 OpenRC。请使用标准 Linux VPS；不支持没有服务管理器的容器或精简系统。"
fi

# Install only missing tools. Do not run a full distribution upgrade.
packages=""
for cmd in curl openssl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then packages="$packages $cmd"; fi
done
for cmd in sha256sum mktemp install; do
  if ! command -v "$cmd" >/dev/null 2>&1; then packages="$packages coreutils"; break; fi
done
if ! command -v awk >/dev/null 2>&1; then packages="$packages gawk"; fi
if [ ! -s /etc/ssl/certs/ca-certificates.crt ] && [ ! -s /etc/pki/tls/certs/ca-bundle.crt ]; then
  packages="$packages ca-certificates"
fi
if [ -n "$packages" ]; then
  echo "Installing VPS prerequisites:$packages" >&2
  # packages contains only the fixed package names above; word splitting is intentional.
  # shellcheck disable=SC2086
  if command -v apt-get >/dev/null 2>&1; then
    (apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y $packages) >&2 || harbor_fail "安装依赖失败，请检查 apt 软件源、DNS 和包管理器锁。"
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y $packages >&2 || harbor_fail "安装依赖失败，请检查 dnf 软件源、DNS 和包管理器锁。"
  elif command -v yum >/dev/null 2>&1; then
    yum install -y $packages >&2 || harbor_fail "安装依赖失败，请检查 yum 软件源、DNS 和包管理器锁。"
  elif command -v apk >/dev/null 2>&1; then
    apk add --no-cache $packages >&2 || harbor_fail "安装依赖失败，请检查 apk 软件源、DNS 和包管理器锁。"
  elif command -v pacman >/dev/null 2>&1; then
    pacman -S --needed --noconfirm $packages >&2 || harbor_fail "安装依赖失败，请先在 VPS 上完成系统更新，再重试部署。"
  elif command -v zypper >/dev/null 2>&1; then
    zypper --non-interactive install $packages >&2 || harbor_fail "安装依赖失败，请检查 zypper 软件源和包管理器锁。"
  else
    harbor_fail "无法自动安装依赖，请先手动安装：$packages。支持 apt/dnf/yum/apk/pacman/zypper。"
  fi
fi
for cmd in curl openssl sha256sum awk grep mktemp install; do
  command -v "$cmd" >/dev/null 2>&1 || harbor_fail "缺少必要工具：$cmd，请手动安装后重试。"
done

harbor_stop() {
  if [ "$HARBOR_INIT" = systemd ]; then systemctl stop "$SERVICE"; else rc-service "$SERVICE" stop; fi
}
harbor_disable() {
  if [ "$HARBOR_INIT" = systemd ]; then systemctl disable "$SERVICE"; else rc-update del "$SERVICE" default; fi
}
harbor_start() {
  if [ "$HARBOR_INIT" = systemd ]; then
    systemctl enable "$SERVICE" >&2 && systemctl restart "$SERVICE" >&2 && sleep 1 && systemctl is-active --quiet "$SERVICE"
  else
    rc-update add "$SERVICE" default >&2 && rc-service "$SERVICE" restart >&2 && sleep 1 && rc-service "$SERVICE" status >&2
  fi
}
harbor_start_checked() {
  if ! harbor_start; then
    if [ "$HARBOR_INIT" = systemd ]; then
      journalctl -u "$SERVICE" --no-pager -n 20 >&2 || true
    else
      tail -n 20 /var/log/hysteria-server.log >&2 || true
    fi
    harbor_fail "Hysteria2 服务启动失败。请检查 543/UDP 端口占用、配置和服务日志。"
  fi
}

harbor_is_running() {
  if [ "$HARBOR_INIT" = systemd ]; then
    systemctl is-active --quiet "$SERVICE"
  else
    rc-service "$SERVICE" status >/dev/null 2>&1
  fi
}

harbor_is_enabled() {
  if [ "$HARBOR_INIT" = systemd ]; then
    systemctl is-enabled --quiet "$SERVICE"
  else
    rc-update show default 2>/dev/null | grep -Eq "(^|[[:space:]])${SERVICE}([[:space:]]|$)"
  fi
}

harbor_reload_init() {
  if [ "$HARBOR_INIT" = systemd ]; then systemctl daemon-reload; fi
}
