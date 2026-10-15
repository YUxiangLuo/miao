#!/usr/bin/env bash
# Miao 独立卸载入口，可直接下载执行。
set -euo pipefail

BIN_PATH=/usr/local/bin/miao
CONFIG_DIR=/etc/miao
RUNTIME_DIR=/tmp/miao-sing-box
UNIT_PATH=/etc/systemd/system/miao.service

log() { echo "==> $*"; }

usage() {
  cat <<'EOF'
用法: sudo bash remove.sh [-y|--yes]
  删除服务、二进制、/etc/miao、/tmp/miao-sing-box，清理残留内核和 TUN。
  先备份配置；非交互执行必须传 -y，其他情况在终端确认。
  -h, --help  显示帮助，无需 root。
EOF
}

# 交互确认;-y/--yes 跳过。管道等非交互场景必须显式加 -y,否则拒绝执行
skip_confirm=no
for arg in "$@"; do
  case "$arg" in
    -y|--yes) skip_confirm=yes ;;
    -h|--help) usage; exit 0 ;;
    *) echo "未知参数: $arg" >&2; usage >&2; exit 1 ;;
  esac
done
if [[ "$(id -u)" -ne 0 ]]; then
  echo "需要 root 权限,请用 sudo 运行" >&2
  exit 1
fi
if [[ "$skip_confirm" != "yes" ]]; then
  echo "将删除 miao 服务、二进制、配置($CONFIG_DIR)与运行时文件。"
  # 先静默探测控制终端是否可用(仅探测时屏蔽 stderr,避免吞掉下面的提示语)
  if : 2>/dev/null < /dev/tty; then
    read -r -p "确认卸载?[y/N] " answer < /dev/tty
    case "$answer" in
      y|Y|yes|YES) ;;
      *) echo "已取消"; exit 0 ;;
    esac
  else
    echo "非交互环境,请显式加 -y 确认:" >&2
    echo "  curl -fsSL https://raw.githubusercontent.com/YUxiangLuo/miao/master/remove.sh | sudo bash -s -- -y" >&2
    exit 1
  fi
fi

if command -v systemctl >/dev/null 2>&1; then
  if systemctl is-active --quiet miao 2>/dev/null; then
    log "停止 miao 服务"
    systemctl stop miao
  fi
  if systemctl is-enabled --quiet miao 2>/dev/null; then
    log "禁用开机自启"
    systemctl disable miao
  fi
fi
if [[ -f "$UNIT_PATH" ]]; then
  rm -f "$UNIT_PATH"
  log "已删除 systemd 单元"
fi
command -v systemctl >/dev/null 2>&1 && systemctl daemon-reload || true

# 兜底清理:服务异常退出时可能残留的 sing-box 进程与 TUN 网卡
if pgrep -f "$RUNTIME_DIR/sing-box" >/dev/null 2>&1; then
  log "清理残留的 sing-box 进程"
  pkill -TERM -f "$RUNTIME_DIR/sing-box" || true
  sleep 1
  pgrep -f "$RUNTIME_DIR/sing-box" >/dev/null 2>&1 && pkill -KILL -f "$RUNTIME_DIR/sing-box" || true
fi
if command -v ip >/dev/null 2>&1 && ip link show sing-tun >/dev/null 2>&1; then
  log "删除残留的 sing-tun 网卡"
  ip link delete sing-tun || true
fi

if [[ -e "$BIN_PATH" || -e "${BIN_PATH}.bak" ]]; then
  rm -f "$BIN_PATH" "${BIN_PATH}.bak"
  log "已删除二进制 $BIN_PATH"
fi
if [[ -d "$CONFIG_DIR" ]]; then
  rm -rf "$CONFIG_DIR"
  log "已删除配置目录 $CONFIG_DIR"
fi
if [[ -d "$RUNTIME_DIR" ]]; then
  rm -rf "$RUNTIME_DIR"
  log "已删除运行时目录 $RUNTIME_DIR"
fi

log "卸载完成"
