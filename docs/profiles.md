# Profile 与路径归属

[`ResolvedProfile`](../crates/harbor-core/src/profile.rs) 在启动时一次性解析全部文件路径，服务运行期仅使用 `AppState` 中的结果。**Profile 隔离不支持多开**：TUN、Clash API（`:6262`）等进程级资源仍共享。

## 启动来源

| Profile | 触发方式 | 行为 |
| --- | --- | --- |
| 默认 | 无参启动，或 `--config` 指向默认发现文件 | 优先可执行文件旁的 `config.yaml`（Windows 大小写不敏感），否则平台默认路径 |
| 命名 | `--config PATH` 或 SDK 指定其他路径 | 状态独立；缺失文件时先用内存默认值，首次保存才创建 |
| 临时 | `--sub URL [地区]` | 配置、运行态、缓存与偏好均在 `0700` 临时目录内，不继承已有状态 |

路径统一转为绝对路径并解析符号链接（尚不存在的路径解析最近现存祖先；保存写入目标路径，不覆盖符号链接）。命名 Profile 的 `<id>` 为规范化完整路径的 SHA-256（Unix 原生字节、Windows UTF-16LE 字节），改名或移动即视为新 Profile。

## 文件位置

约定：`R` 为运行根目录（Linux/OpenWrt `/tmp/harbor-sing-box`，Windows `%TEMP%\harbor-sing-box`）；`D` 为 Windows 数据目录 `%LOCALAPPDATA%\io.github.yuxiangluo.harbor`；`P` 为配置同目录的 `.harbor-profiles/<id>`。

| 文件与用途 | 默认 Profile | 命名 Profile |
| --- | --- | --- |
| `config.yaml`：稳定配置（订阅、节点、规则、终端、独立端口、MCP、定时刷新、启动默认值） | 上述发现路径 | 指定路径 |
| 派生运行材料（内核、active `config.json`、缓存、订阅快照） | `R` | `R/profiles/<id>` |
| `volatile.yaml`：易变状态（策略、倍率、路由模式、禁用节点） | Linux/OpenWrt：`R`<br>Windows：`D` | Linux/OpenWrt：`R/profiles/<id>`<br>Windows：`P` |
| `.last_proxy` / `.node_select` / `.max_multiplier`：显式选择偏好 | Windows：`D`<br>systemd Linux：启动 CWD（服务为 `/etc/harbor`）<br>OpenWrt / 非 systemd：`R` | Windows / systemd Linux：`P`<br>OpenWrt / 非 systemd：`R/profiles/<id>` |
| `node-bindings.json`：稳定节点 tag 映射 | 配置同目录 | 文件名为 `config.yaml` 时同目录，否则 `P` |
| `harbor.log`：体积封顶滚动日志（2 MiB + 单份 `.old`） | Windows：`D/harbor.log`<br>OpenWrt / 非 systemd：`R/harbor.log`<br>systemd Linux：输出终端（journald） | Windows：`D/harbor.log`<br>OpenWrt / 非 systemd：`R/profiles/<id>/harbor.log`<br>systemd Linux：输出终端 |

- Linux/OpenWrt 的 `R` 默认在 tmpfs（进程重启保留、系统重启清空）；Windows 易变层与偏好持久落盘；`config.yaml` 与 `node-bindings.json` 始终持久保存。
- 旧版兼容：命名 Profile 目标绑定文件缺失时，若同目录存在旧版 `<name>.node-bindings.json` 会原子复制迁移；旧易变层中显式记录的倍率与 `fastest_*` 在首次升级时迁移为偏好文件。

## SDK 与生命周期

- `spawn_server(RuntimeOptions)` 不读取宿主 argv；`config_path: None` 走默认发现。
- `runtime_dir` 覆盖运行目录、默认偏好与默认易变层（Windows 上亦覆盖默认日志目录）；显式 `volatile_path` / `log_path` 优先级更高。调用方传入的目录不由 runtime 删除。
- 临时 `--sub` 目录由 `AppState` 持有，待所有请求与后台任务释放引用后自动删除（启动失败同样清理；强杀/断电/exec 升级不保证清理，后续运行也不会复用残留目录）。
- `ServerHandle::shutdown()` 等待已接收写任务（含已断连客户端的写请求）收尾并停核；直接 drop 异步执行同一流程。隔离测试约定见[开发指南](development.md#测试隔离与维护约定)。
