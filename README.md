# Miao

一个开箱即用的 [sing-box](https://github.com/SagerNet/sing-box) 管理器。下载、配置、运行，即可实现 **TUN 模式透明代理 + 国内外自动分流**。

> ⚠️ **当前仅支持 Hysteria2 协议节点**

## 🚀 30 秒快速开始

### 1. 创建目录并下载

```bash
# 创建工作目录
mkdir ~/miao && cd ~/miao

# 下载 (根据你的架构选择一个)

# Linux amd64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao
chmod +x miao

# Linux arm64 (树莓派、软路由等)
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-arm64 -O miao
chmod +x miao
```

### 2. 创建配置文件

在**同一目录**下创建 `config.yaml`：

```yaml
port: 6161
subs:
  - "https://your-hysteria2-subscription-url"
```

或者手动配置节点：

```yaml
port: 6161
nodes:
  # 基础配置 (SNI 默认使用 server 地址)
  - '{"type":"hysteria2","tag":"节点1","server":"example.com","server_port":443,"password":"xxx"}'
  
  # 指定 SNI (当 server 是 IP 时需要)
  - '{"type":"hysteria2","tag":"节点2","server":"1.2.3.4","server_port":443,"password":"xxx","sni":"example.com"}'
```

### 3. 运行

```bash
sudo ./miao
```

> 需要 root 权限创建 TUN 网卡

### 4. 完成！🎉

<img width="2404" height="1435" alt="image" src="https://github.com/user-attachments/assets/d71c581d-e74e-477c-b97b-d3f992c77bb9" />
