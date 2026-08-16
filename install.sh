#!/usr/bin/env bash
# Miao 一键安装:下载最新 release 并注册为 systemd 服务(仅 Linux + systemd)
#
# 用法:
#   curl -fsSL https://raw.githubusercontent.com/YUxiangLuo/miao/master/install.sh | sudo bash
#   sudo bash install.sh /path/to/miao-binary   # 离线安装本地二进制(网络受限时先另行下载)
#   curl -fsSL https://miao.vesein.dev/install.sh | sudo MIAO_BASE_URL=https://miao.vesein.dev/dl bash   # 镜像站(无法访问 GitHub 时)
# 重复运行即升级到最新版本。
set -euo pipefail

BIN_PATH=/usr/local/bin/miao
CONFIG_DIR=/etc/miao
UNIT_PATH=/etc/systemd/system/miao.service
REPO=YUxiangLuo/miao
LOCAL_BIN="${1:-}"
# 二进制下载基地址;无法访问 GitHub 时可用镜像站
BASE_URL="${MIAO_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
# 卸载脚本地址(随下载基地址切换)
if [[ -n "${MIAO_BASE_URL:-}" ]]; then
  REMOVE_SH_URL="${MIAO_BASE_URL%/dl}/remove.sh"
else
  REMOVE_SH_URL="https://raw.githubusercontent.com/$REPO/master/remove.sh"
fi

log() { echo "==> $*"; }

if [[ "$(id -u)" -ne 0 ]]; then
  echo "需要 root 权限,请用 sudo 运行:" >&2
  echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/master/install.sh | sudo bash" >&2
  exit 1
fi

if [[ -z "$LOCAL_BIN" ]]; then
  case "$(uname -m)" in
    x86_64|amd64) asset_arch=amd64 ;;
    aarch64|arm64) asset_arch=arm64 ;;
    *)
      echo "不支持的架构: $(uname -m)(仅支持 amd64 / arm64)" >&2
      exit 1
      ;;
  esac

  if command -v curl >/dev/null 2>&1; then
    download() { curl --fail --location --retry 3 --progress-bar "$1" -o "$2"; }
  elif command -v wget >/dev/null 2>&1; then
    download() { wget -q --show-progress "$1" -O "$2"; }
  else
    echo "需要 curl 或 wget 来下载二进制文件" >&2
    exit 1
  fi
else
  if [[ ! -f "$LOCAL_BIN" ]]; then
    echo "本地二进制不存在: $LOCAL_BIN" >&2
    exit 1
  fi
fi

if ! command -v systemctl >/dev/null 2>&1; then
  echo "未找到 systemctl:本脚本只支持 systemd 系统(OpenWrt 请直接运行二进制)" >&2
  exit 1
fi

stop_miao_service() {
  if systemctl is-active --quiet miao 2>/dev/null; then
    log "检测到正在运行的 miao 服务,先停止以便升级"
    systemctl stop miao
  fi
}

# 本地安装时先停服务,避免 6161 仍被当前实例占用而误报端口冲突
if [[ -n "$LOCAL_BIN" ]]; then
  stop_miao_service
fi

# 预检:面板端口被占用(例如另一个非 systemd 的实例)时提前报错,而不是装完反复重启失败
if command -v ss >/dev/null 2>&1 && ss -tlnH '( sport = :6161 )' | grep -q .; then
  echo "端口 6161 已被占用,请先停止占用进程(或先运行 remove.sh 卸载旧实例)" >&2
  exit 1
fi

# 在线安装升级:替换二进制前同样先停服务
if [[ -z "$LOCAL_BIN" ]]; then
  stop_miao_service
fi

tmp_file=$(mktemp)
trap 'rm -f "$tmp_file"' EXIT

if [[ -n "$LOCAL_BIN" ]]; then
  log "使用本地二进制: $LOCAL_BIN"
  cp "$LOCAL_BIN" "$tmp_file"
else
  log "下载最新 release(linux-$asset_arch)..."
  download "$BASE_URL/miao-rust-linux-$asset_arch" "$tmp_file"
fi

# 完整性自检:必须是 ELF 可执行文件(挡下载损坏或被镜像劫持返回的错误页)
if [[ "$(head -c 4 "$tmp_file")" != $'\x7fELF' ]]; then
  echo "下载/提供的文件不是有效的可执行文件(ELF),已中止" >&2
  exit 1
fi

log "安装到 $BIN_PATH"
install -m 755 "$tmp_file" "$BIN_PATH"
rm -f "${BIN_PATH}.bak"

mkdir -p "$CONFIG_DIR"
chmod 750 "$CONFIG_DIR"

log "写入 systemd 单元 $UNIT_PATH"
cat > "$UNIT_PATH" <<'EOF'
[Unit]
Description=Miao transparent proxy (embedded sing-box)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/miao
WorkingDirectory=/etc/miao
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

log "启用并启动 miao 服务"
systemctl daemon-reload
systemctl enable --now miao

sleep 1
if systemctl is-active --quiet miao; then
  log "完成!面板地址: http://localhost:6161"
  echo
  echo "常用命令:"
  echo "  systemctl status miao    # 查看状态"
  echo "  journalctl -u miao -f    # 查看日志"
  echo "  卸载: curl -fsSL $REMOVE_SH_URL | sudo bash"
else
  echo "服务启动失败,请用 journalctl -u miao -e 查看日志" >&2
  exit 1
fi
