# Miao

开箱即用的透明代理分流器，基于 sing-box。单文件、零依赖，支持 Linux 与 OpenWrt。

![screenshot](https://github.com/user-attachments/assets/172530bf-cb7e-4482-8dfd-ea8146c33eb0)

## 快速开始

下载对应架构的可执行文件：

```bash
mkdir -p ~/miao && cd ~/miao

# amd64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao

# arm64
# wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-arm64 -O miao

chmod +x miao
sudo ./miao
```

启动后访问：

```text
http://localhost:6161
```

首次启动会进入引导页，添加订阅或手动节点后即可使用。

## 智能助手（MVP）

点击 Miao Logo 旁的对话按钮可按需启动 Pi Agent。首次使用时，Miao 会检查 `/tmp` 空间和可用内存，下载并校验固定兼容版本的 Pi；Pi 进程在对话框关闭或空闲超时后退出，下载文件暂存在 `/tmp`（通常随系统重启清理）。

当前 MVP 的安全与兼容范围：

- 仅允许通过本机 loopback（如 `localhost` 或 `127.0.0.1`）访问助手 API
- 仅支持 API Key Provider，不支持 Pi 的 OAuth/订阅登录
- Pi 以无内置工具、无会话持久化模式运行，并在 Miao 以 root 运行时使用临时低权限 UID
- Provider 密钥保存在配置文件同目录的 `.miao-agent/credentials.json`，权限为 `0600`
- 官方 Pi Linux 二进制依赖 glibc；原生 OpenWrt/musl 暂不支持
- 首次安装要求 `/tmp` 至少有 256 MiB 可用空间和 64 个可用 inode，并要求至少 512 MiB 可用内存

完整的信任边界、自定义 WebSocket 协议和剩余风险见 [Pi Agent MVP 安全说明](docs/pi-agent-security.md)。

## 配置文件

Miao 按以下顺序查找配置：

1. `--config /path/to/config.yaml`
2. 可执行文件同目录的 `config.yaml`
3. `/etc/miao/config.yaml`

如果没有配置文件，会使用内存默认配置并进入引导页；只有在面板中添加订阅、节点或触发持久化变更时才会写入配置。

示例：

```yaml
port: 6161

subs:
  - "https://your-subscription-url"

nodes:
  - '{"type":"hysteria2","tag":"HY2","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
```

运行时文件位于：

```text
/tmp/miao-sing-box
```

## 可选：自动初始化 VPS

如果当前 root 环境可免密 SSH 登录目标 VPS，可以在配置中加入：

```yaml
vps_ip: "203.0.113.10"
```

Miao 会尝试在该 VPS 上部署 Hysteria2，并把生成的节点写回本地配置。

部署前建议先测试：

```bash
sudo ssh -o BatchMode=yes root@203.0.113.10 true
```

## 从源码构建

需要安装 Bun、Go、Rust 和 curl。构建当前机器架构：

```bash
./build.sh
```

`build.sh` 只构建当前机器架构。跨架构构建由 GitHub Actions 负责。构建脚本会使用同一套流程准备前端、sing-box 和 geo 规则集；可以通过 `SING_BOX_REF` 指定 sing-box 的分支或 tag。

构建产物：

```text
target/release/miao-rust
```
