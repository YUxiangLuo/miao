# Miao

开箱即用的透明代理。

<img width="1415" height="952" alt="image" src="https://github.com/user-attachments/assets/172530bf-cb7e-4482-8dfd-ea8146c33eb0" />

## 快速开始

```bash
mkdir ~/miao && cd ~/miao

# amd64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao && chmod +x miao
# arm64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-arm64 -O miao && chmod +x miao
```

创建 `config.yaml`：

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

运行（需要 root 权限以创建 TUN 网卡）：

```bash
sudo ./miao
```

访问 `http://localhost:6161` 进入控制面板。
