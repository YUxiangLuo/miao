# Miao

一个开箱即用的透明代理与国内外分流启动器，基于 sing-box 内核。

Miao 旨在为用户提供零配置的透明代理体验（TUN 模式）。它内部集成了 sing-box 核心、预置了分流规则（中国大陆/国外自动分流），并提供了一个现代化的 Web 控制面板来进行订阅和节点管理。支持在普通 Linux 系统以及 OpenWrt 路由器上运行。

## 🌟 核心特性
- **开箱即用**: 内嵌 sing-box 核心与 GEOIP/GEOSITE 分流规则，零依赖，下载单文件即用。
- **现代化 Web 面板**: 基于 React + Vite 构建的单页应用，提供直观的订阅管理、节点测速和实时流量状态监控。
- **强大的协议支持**: 原生支持当下主流的代理协议，包括 Hysteria2、AnyTLS 和 Shadowsocks(SS) 等。
- **透明代理 (TUN)**: 自动配置系统的 TUN 网卡以接管全局流量，真正无感知的透明代理体验。
- **自动分流**: 内置强大的中国大陆国内直连、海外流量走代理的自动规则。
- **跨平台与 OpenWrt 支持**: 提供不同 CPU 架构（amd64, arm64）的版本，并在 OpenWrt 环境下能自动安装所需内核依赖。

## 🚀 快速开始

### 1. 下载与安装
创建一个专门的目录来存放应用和配置：
```bash
mkdir ~/miao && cd ~/miao
```

根据您的系统架构下载最新版本的可执行文件并赋予执行权限：
```bash
# Linux amd64 (常见 PC 和服务器)
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64 -O miao && chmod +x miao

# Linux arm64 (树莓派、Arm 软路由、服务器等)
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-arm64 -O miao && chmod +x miao
```

### 2. 初始化配置
在程序同级目录下创建配置清单文件 `config.yaml`。您可以通过以下两种方式配置节点：

**方式一：添加订阅链接（推荐）**
支持多个订阅地址，建议使用 Clash.Meta 格式的订阅链接：
```yaml
port: 6161 # 可选：Web 面板访问端口，默认为 6161
subs:
  - "https://your-hysteria2-subscription-url"
  - "https://your-anytls-subscription-url"
```

**方式二：手动配置节点**
如果您没有订阅，也可以直接通过 JSON 格式填写节点信息：
```yaml
nodes:
  # Hysteria2 节点示例
  - '{"type":"hysteria2","tag":"Hy2节点","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
  
  # AnyTLS 节点示例
  - '{"type":"anytls","tag":"AnyTLS节点","server":"example.com","server_port":443,"password":"xxx","tls":{"enabled":true}}'
  
  # Shadowsocks 节点示例
  - '{"type":"shadowsocks","tag":"SS节点","server":"example.com","server_port":443,"method":"2022-blake3-aes-128-gcm","password":"xxx"}'
```

*(提示：您既可以只配置 `subs`，也可以只配置 `nodes`，或者两者混合使用)*

### 3. 运行程序
Miao 需要以超级用户 (root) 权限运行，以便创建虚拟 TUN 网卡和管理系统路由：
```bash
sudo ./miao
```

运行成功后，您可以打开浏览器访问 `http://localhost:6161` (或您配置的自定义端口) 进入 Miao 控制面板。

## ⚙️ 进阶说明

- **面板功能**: 
  - 通过 Web 面板动态更新订阅。
  - 测试节点延迟并切换当前代理节点。
  - 自动静默更新应用至 Github 发布的最新 Release 版本。
- **OpenWrt 特别说明**: 在 OpenWrt 路由器上，初次运行时程序会自动检测并尝试安装 TUN 工作所需的基础包与内核模块，大幅降低软路由的配置门槛。
