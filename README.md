# Miao

<p align="center">
  <img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/sing--box-00ADD8?style=for-the-badge&logo=go&logoColor=white" alt="sing-box">
  <img src="https://img.shields.io/badge/Vue.js-35495E?style=for-the-badge&logo=vuedotjs&logoColor=4FC08D" alt="Vue.js">
</p>

一个轻量级的 [sing-box](https://github.com/SagerNet/sing-box) 管理器，使用 Rust 编写，内嵌 sing-box 二进制，开箱即用。

## ✨ 特性

- 🚀 **开箱即用** - 内嵌 sing-box 二进制，无需额外安装
- 🌐 **Web 控制面板** - 现代化 Vue 3 界面，支持中文
- 📡 **订阅管理** - 支持 Clash 格式订阅链接
- 🔧 **节点管理** - 可通过表单手动添加 Hysteria2 节点
- 📦 **单文件部署** - 编译后仅一个可执行文件
- 🖥️ **多架构支持** - 支持 amd64 和 arm64
- 🔄 **自动更新** - 每 6 小时自动刷新订阅配置

## 📥 安装

### 下载预编译版本

从 [Releases](https://github.com/YUxiangLuo/miao/releases) 下载对应架构的二进制文件：

```bash
# Linux amd64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-amd64
chmod +x miao-rust-linux-amd64

# Linux arm64
wget https://github.com/YUxiangLuo/miao/releases/latest/download/miao-rust-linux-arm64
chmod +x miao-rust-linux-arm64
```

### 从源码编译

```bash
git clone https://github.com/YUxiangLuo/miao.git
cd miao
cargo build --release
```

> **注意**: 从源码编译需要在 `embedded/` 目录下放置对应架构的 sing-box 二进制文件。

## 🚀 快速开始

1. **创建配置文件**

```bash
cp miao.yaml.example miao.yaml
```

2. **编辑配置**

```yaml
# miao.yaml
port: 6161

# 订阅链接
subs:
  - https://your-subscription-url.com/sub

# 手动节点 (可选)
nodes: []
```

3. **启动服务**

```bash
./miao-rust-linux-amd64
```

4. **访问控制面板**

打开浏览器访问 `http://localhost:6161`

## 📖 配置说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|:----:|------|
| `port` | number | ✅ | HTTP API 端口 |
| `sing_box_home` | string | ❌ | 自定义 sing-box 目录，默认使用当前目录 |
| `subs` | array | ❌ | 订阅链接列表 |
| `nodes` | array | ❌ | 手动节点 JSON 配置 |

## 🔌 API 接口

### 服务状态

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/status` | 获取 sing-box 运行状态 |
| POST | `/api/service/start` | 启动 sing-box |
| POST | `/api/service/stop` | 停止 sing-box |
| POST | `/api/service/restart` | 重启 sing-box |

### 配置管理

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/config` | 获取当前 sing-box 配置 |
| POST | `/api/config/generate` | 重新生成配置 |

### 订阅管理

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/subs` | 获取订阅列表 |
| POST | `/api/subs` | 添加订阅 `{"url": "..."}`|
| DELETE | `/api/subs` | 删除订阅 `{"url": "..."}` |

### 节点管理

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/nodes` | 获取手动节点列表 |
| POST | `/api/nodes` | 添加节点 |
| DELETE | `/api/nodes` | 删除节点 `{"tag": "..."}` |

**添加节点请求体示例:**
```json
{
  "tag": "我的节点",
  "server": "example.com",
  "server_port": 443,
  "password": "your-password",
  "sni": "optional-sni.com"
}
```

## 🖥️ OpenWrt 部署

Miao 会自动检测 OpenWrt 系统并安装必要依赖：

- `kmod-tun`
- `kmod-nft-queue`

## 📸 截图

控制面板提供以下功能：

- 🟢 服务状态监控（PID、运行时间）
- ▶️ 启动/停止/重启服务
- 📋 订阅管理（添加/删除）
- ➕ Hysteria2 节点表单（手动添加）
- 📄 配置查看

## 🛠️ 技术栈

- **后端**: Rust + Axum + Tokio
- **前端**: Vue 3 (CDN) + Bootstrap 5
- **代理核心**: sing-box

## 📄 许可证

MIT License

## 🙏 致谢

- [sing-box](https://github.com/SagerNet/sing-box) - 通用代理平台
