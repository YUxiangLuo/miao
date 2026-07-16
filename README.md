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
