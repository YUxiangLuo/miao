# Profile 与路径归属

[`profile::ResolvedProfile`](../crates/miao-core/src/profile.rs) 在启动时统一解析配置、运行文件、易变层、偏好和日志路径。服务使用 `AppState` 中的结果，不按当前目录或进程参数重新猜测。

## 启动来源

| Profile | 来源与行为 |
| --- | --- |
| 默认 | 可执行文件旁的 `config.yaml`，否则平台默认配置；显式指定同一文件与默认启动等价 |
| 命名 | `--config PATH` 或 SDK 选择其他配置；状态独立，文件不存在时先用内存默认值，写入时再保存 |
| 临时 | `--sub URL [地区]`，配置和所有状态属于独立临时目录，不继承已有配置、缓存或偏好 |

路径规范化为绝对路径并解析符号链接；未创建的路径解析最近的现存祖先。保存写入解析后的目标，不替换符号链接；第一次保存不会改变归属。命名 Profile 的 `<id>` 是完整路径的 SHA-256（Unix 原生字节、Windows UTF-16LE），不同扩展名/目录互不混用；移动或改名配置会选择另一个 Profile。

## 文件位置

默认配置为 Linux/OpenWrt 的 `/etc/miao/config.yaml`，或 Windows 的 `%LOCALAPPDATA%\io.github.yuxiangluo.miao\config.yaml`；可执行文件旁的配置优先。

以下 `R` 表示内核运行根目录（Unix：`/tmp/miao-sing-box`；Windows：`%TEMP%\miao-sing-box`），`A` 为 Windows 应用数据目录，`P` 为配置同目录的 `.miao-profiles/<id>`。

| 文件 | 默认 Profile | 命名 Profile |
| --- | --- | --- |
| 内核、active config、缓存、订阅快照 | `R` | `R/profiles/<id>` |
| `volatile.yaml` | Unix：`R`；Windows：`A` | Unix：该 Profile 的运行目录；Windows：`P` |
| `.last_proxy` / `.node_select` / `.max_multiplier` | systemd Linux：启动 CWD（安装服务为 `/etc/miao`）；OpenWrt/非 systemd：`R`；Windows：`A` | systemd Linux / Windows：`P`；OpenWrt/非 systemd：该 Profile 的运行目录 |
| `node-bindings.json` | 配置同目录 | 配置名为 `config.yaml` 时沿用同目录文件；其他文件名使用 `P` |

OpenWrt/非 systemd 的高频状态在 tmpfs，系统重启后消失；稳定配置和节点绑定跟随配置文件。默认 Profile 的 CWD 偏好规则为兼容保留，命名 Profile 不依赖启动 CWD。

Windows 默认日志为 `A/miao.log`，供托盘打开；临时 Profile 使用自身目录。Unix 默认输出终端。

## 迁移

默认路径不变。命名 Profile 不继承旧共享偏好、易变层或缓存，也不删除旧文件，初次使用按自身 YAML 默认值启动。

旧版 `travel.node-bindings.json` 等绑定文件在新文件不存在时原子复制到 `P/node-bindings.json`，保留 tag；旧文件不删，新文件不覆盖。之后不同 Profile 分别写入各自绑定，即使旧版曾因相同文件 stem 共用文件。

## SDK 与生命周期

- `spawn_server(RuntimeOptions)` 不读取宿主 argv；`config_path: None` 只做默认发现。CLI 和桌面共用参数解析器，支持 `--config PATH` / `--config=PATH`。
- `runtime_dir` 覆盖运行文件和默认偏好目录，各平台默认易变层也随之移动，Windows 默认日志同样隔离；显式 `volatile_path` / `log_path` 优先。
- 调用方提供的目录不由 runtime 删除。临时 `--sub` 使用 Unix `0700` 目录，所有权交给 `AppState`，最后一个请求/后台任务引用释放后才清理；启动失败同样释放所有权。
- `ServerHandle::shutdown()` 等待服务和内核关闭；仅 drop handle 则异步停服，均不提前删除仍在使用的目录。强杀、断电和 exec 升级不保证析构清理。

**Profile 隔离不支持多开**：TUN、Clash API 端口和部分进程级设施仍共享。测试隔离要求见[开发指南](../DEV_NOTES.md#开发检查)。
