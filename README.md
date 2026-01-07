# Miao

一个开箱即用的 [sing-box](https://github.com/SagerNet/sing-box) 启动器。下载、配置订阅、运行，即可实现 **TUN 模式透明代理 + 国内外自动分流**。

> **当前支持 Hysteria2、AnyTLS 和 Shadowsocks (SS) 协议节点**

## 特性

- **零配置 sing-box** - 内嵌 sing-box 二进制，无需单独安装
- **TUN 透明代理** - 系统级代理，所有流量自动走代理
- **国内外自动分流** - 基于 geosite/geoip 规则，国内直连、国外代理
- **Web 管理面板** - 节点管理、订阅管理、实时流量监控、测速
- **自动更新** - 支持从 GitHub 一键更新到最新版本
- **OpenWrt 支持** - 自动安装所需内核模块

<img width="2560" height="1440" alt="image" src="https://github.com/user-attachments/assets/e5e101c1-6002-423b-956a-e4730c67bc12" />

## 快速开始

### 1. 下载

```bash
mkdir ~/miao && cd ~/miao

# Linux amd64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao && chmod +x miao

# Linux arm64 (树莓派、路由器等)
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-arm64 -O miao && chmod +x miao
```

### 2. 配置

在同一目录下创建 `config.yaml`：

```yaml
# 订阅方式
subs:
  - "https://your-hysteria2-subscription-url"
  - "https://your-hysteria2-subscription-url2"
  - "https://your-hysteria2-subscription-url3"
```

或手动配置节点：

```yaml
# 手动配置节点
nodes:
  - '{"type":"hysteria2","tag":"节点名","server":"example.com","server_port":443,"password":"xxx"}'
  # server 是 IP 时需指定 sni
  - '{"type":"hysteria2","tag":"节点2","server":"1.2.3.4","server_port":443,"password":"xxx","sni":"example.com"}'
```

### 3. 运行

```bash
sudo ./miao
```

访问 `http://localhost:6161` 打开管理面板。

## 配置说明

| 字段 | 说明 | 默认值 |
|------|------|--------|
| `port` | Web 面板端口 | `6161` |
| `subs` | 订阅 URL 列表 | - |
| `nodes` | 手动配置的节点 (JSON 格式) | - |
