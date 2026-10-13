# Profile 与路径归属

[`ResolvedProfile`](../crates/miao-core/src/profile.rs) 在启动时解析所有路径，服务只使用 `AppState` 中的结果。**Profile 隔离不支持多开**：TUN、Clash API 端口和部分进程级设施仍共享。

## 启动来源

| Profile | 来源与行为 |
| --- | --- |
| 默认 | 可执行文件旁的 `config.yaml`，否则平台默认路径；显式指定同一文件等价 |
| 命名 | `--config PATH` 或 SDK 指定其他配置；状态独立，缺失文件先用内存默认值，首次保存才创建 |
| 临时 | `--sub URL [地区]`；配置、缓存、偏好均在独立临时目录，不继承已有状态 |

路径转为绝对路径并解析符号链接，尚未创建的路径解析最近现存祖先；保存写入解析后的目标，不替换符号链接。命名 Profile 的 `<id>` 为完整路径 SHA-256（Unix 原生字节、Windows UTF-16LE），移动或改名会选择新 Profile，首次保存不改变归属。

## 文件位置

平台默认配置：Linux/OpenWrt 为 `/etc/miao/config.yaml`；Windows 为 `%LOCALAPPDATA%\io.github.yuxiangluo.miao\config.yaml`。可执行文件旁的配置优先。

`R` 为运行根目录（Unix `/tmp/miao-sing-box`；Windows `%TEMP%\miao-sing-box`），`A` 为上述 Windows 应用数据目录，`P` 为配置同目录的 `.miao-profiles/<id>`。

| 文件与用途 | 默认 Profile | 命名 Profile |
| --- | --- | --- |
| `config.yaml`：订阅、手动节点、规则、端口、MCP、定时刷新与启动默认值 | 上述发现路径 | 指定路径 |
| 内核、active `config.json`、运行缓存、订阅快照：派生运行材料 | `R` | `R/profiles/<id>` |
| `volatile.yaml`：当前策略、倍率、路由模式、禁用节点 | Unix `R`；Windows `A` | Unix 该 Profile 运行目录；Windows `P` |
| `.last_proxy` / `.node_select` / `.max_multiplier`：显式选择偏好 | systemd Linux 启动 CWD（安装服务为 `/etc/miao`）；OpenWrt/非 systemd `R`；Windows `A` | systemd Linux / Windows `P`；OpenWrt/非 systemd 该 Profile 运行目录 |
| `node-bindings.json`：稳定节点 tag | 配置同目录 | 配置名为 `config.yaml` 时沿用同目录文件，其他名称使用 `P` |

默认 Unix 易变层及 OpenWrt/非 systemd 的偏好在 tmpfs，进程重启保留、系统重启清空；稳定配置与节点绑定持久保存。默认 Profile 的 CWD 偏好规则为兼容保留，命名 Profile 不依赖 CWD。Windows 默认日志为 `A/miao.log`，临时 Profile 使用自身目录；Unix 默认输出终端。

## 迁移

默认路径不变。命名 Profile 不继承或删除旧共享状态，初次按自身 YAML 启动。旧 `travel.node-bindings.json` 等绑定文件仅在目标缺失时原子复制到 `P/node-bindings.json`，保留 tag 与旧文件，不覆盖新文件。

首次升级可将旧易变层中明确记录的倍率和 `fastest_*` 迁移为偏好；不提升来源不明的 `manual`。具体手动节点仍由 `.last_proxy` 恢复。

## SDK 与生命周期

- `spawn_server(RuntimeOptions)` 不读取宿主 argv；`config_path: None` 只做默认发现。CLI/桌面共用参数解析器，支持 `--config PATH` / `--config=PATH`。
- `runtime_dir` 覆盖运行文件、默认偏好、默认易变层与 Windows 默认日志；显式 `volatile_path` / `log_path` 优先。调用方提供的目录不由 runtime 删除。
- 临时 `--sub` 在 Unix 使用 `0700` 目录，由 `AppState` 持有，最后一个请求/后台任务引用释放后清理，启动失败同样释放。
- `ServerHandle::shutdown()` 等待服务与内核关闭；仅 drop 则异步停服，均不提前删使用中的目录。强杀、断电、exec 升级不保证清理，后续临时运行不会复用残留目录。

测试注入隔离路径，见[开发检查](../DEV_NOTES.md#开发检查)。
