# Miao

开箱即用的透明代理分流器，基于 sing-box 内核。单文件、零依赖，支持 Linux 与 OpenWrt。

<img width="1415" height="952" alt="image" src="https://github.com/user-attachments/assets/172530bf-cb7e-4482-8dfd-ea8146c33eb0" />

## 快速开始

```bash
mkdir ~/miao && cd ~/miao
# amd64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao && chmod +x miao
```

```bash
mkdir ~/miao && cd ~/miao
# arm64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-arm64 -O miao && chmod +x miao
```

运行（需要 root 权限以创建 TUN 网卡）：

```bash
sudo ./miao
```

访问 `http://localhost:6161`，首次启动会进入引导页面，添加订阅链接或手动节点即可开始使用。

### 进阶：手动编写配置文件

你也可以预先创建 `config.yaml` 跳过引导：

```yaml
port: 6161  # Web 面板端口，默认 6161

# 订阅链接（推荐，Clash.Meta 格式）
subs:
  - "https://your-subscription-url"

# 或手动配置节点（可与 subs 混合使用）
nodes:
  - '{"type":"hysteria2","tag":"HY2","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
  - '{"type":"anytls","tag":"AnyTLS","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
  - '{"type":"shadowsocks","tag":"SS","server":"example.com","server_port":443,"method":"2022-blake3-aes-128-gcm","password":"xxx"}'
```

## 实验性功能

### 自动初始化 Hysteria2 VPS

如果你有一台全新的 VPS，并且当前运行 Miao 的 root 环境可以通过 SSH 私钥免交互登录 `root@<vps_ip>`，可以在 `config.yaml` 中添加：

```yaml
vps_ip: "203.0.113.10"
```

启动时，Miao 会检查 `nodes` 中是否已经存在 `server` 相同的手动节点。不存在时，它会通过 SSH 在该 VPS 上安装 Hysteria2，写入 `/etc/hysteria/config.yaml`，使用 543 端口、自签名证书、随机密码和 Gecko 混淆，然后重启 `hysteria-server.service`。部署成功后，Miao 会把对应的 Hysteria2 手动节点写回本地 `config.yaml`。

如果 `vps_ip` 仍保留，但本地对应的手动节点被删除，Miao 会先尝试通过 SSH 读取远端已有的 `/etc/hysteria/config.yaml` 并恢复本地节点；如果远端配置缺少 Gecko 混淆，Miao 会补写后再恢复本地节点。只有远端没有可复用配置时才重新初始化。

运行前建议先确认：

```bash
sudo ssh -o BatchMode=yes root@203.0.113.10 true
```

如果这条命令失败，自动初始化也会失败。使用 root 运行 Miao 时，SSH 使用的是 `/root/.ssh` 下的密钥和配置。
