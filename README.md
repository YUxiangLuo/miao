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
  - https://your-hysteria2-subscription-url
```

或者手动配置节点：

```yaml
port: 6161
nodes:
  - '{"type":"hysteria2","tag":"我的节点","server":"example.com","server_port":443,"password":"your-password"}'
```

### 3. 运行

```bash
sudo ./miao
```

> 需要 root 权限创建 TUN 网卡

### 4. 完成！🎉

- 访问 `http://localhost:6161` 打开控制面板
- 国内流量直连，国外流量自动走代理
- 支持在网页上添加/删除订阅和节点

**目录结构应该像这样：**
```
~/miao/
├── miao          # 可执行文件
└── config.yaml   # 配置文件 (必须在同一目录)
```

---

## 📱 控制面板功能

- 启动/停止/重启服务
- 添加/删除订阅链接
- 表单添加 Hysteria2 节点
- 查看 sing-box 配置

## 🖥️ OpenWrt 支持

在 OpenWrt 上运行时会自动安装依赖 `kmod-tun` 和 `kmod-nft-queue`。

## 📄 配置文件说明

| 字段 | 说明 |
|------|------|
| `port` | 控制面板端口 |
| `subs` | Hysteria2 订阅链接列表 |
| `nodes` | 手动节点 (JSON 格式) |

## 🔗 API

| 接口 | 说明 |
|------|------|
| `GET /api/status` | 服务状态 |
| `POST /api/service/restart` | 重启服务 |
| `GET/POST/DELETE /api/subs` | 订阅管理 |
| `GET/POST/DELETE /api/nodes` | 节点管理 |

---

## 致谢

- [sing-box](https://github.com/SagerNet/sing-box)
