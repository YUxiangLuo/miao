# Profile 与路径归属

`profile::ResolvedProfile` 在启动时统一决定配置、运行文件、易变层、偏好和日志路径；服务只使用 `AppState` 中已解析的路径，不再按当前工作目录或进程参数临时猜测位置。

## 三种启动来源

- **默认 Profile**：可执行文件旁已有的 `config.yaml`，否则平台默认配置。沿用旧运行目录和平台持久化策略；显式 `--config` 指向这个相同文件时与默认启动等价。
- **命名 Profile**：`--config PATH` 指向其他配置，或 SDK 显式选择其他配置。配置不存在时仍使用内存默认值，不强制创建文件；应用写入时再保存。
- **临时 Profile**：`--sub URL [地区]`。配置及所有状态都在本次创建的独立临时目录中，不读取默认 Profile 的配置、缓存或偏好。

路径先变成绝对路径，并解析已有符号链接；尚未创建的路径解析最近的现存祖先。相对路径、符号链接指向同一配置时使用同一份状态，第一次保存配置不会改变归属。保存写入解析后的目标，不替换配置文件的符号链接。

命名 Profile 的 `<id>` 是解析后完整配置路径的 SHA-256：Unix 使用原生路径字节，Windows 使用 UTF-16LE。不会将非 UTF-8 路径损失性转换成显示字符串，也不会只按去掉扩展名的文件名区分。因此 `travel.yaml`、`travel.yml` 和不同目录下的同名文件各自独立。移动/改名配置视为选择另一个 Profile，不按配置内容猜测或合并状态。

## 默认位置

`R` 表示平台原有内核运行根目录（Unix 通常为 `/tmp/miao-sing-box`），`P` 表示配置文件所在目录下的 `.miao-profiles/<id>`。

| 文件 | 默认 Profile | 命名 Profile |
| --- | --- | --- |
| 内核、active config、运行缓存、订阅节点快照、Clash cache | `R` | `R/profiles/<id>` |
| 易变层 `volatile.yaml` | Unix：`R`；Windows：应用数据目录 | Unix：该 Profile 的运行目录；Windows：`P` |
| `.last_proxy` / `.node_select` / `.max_multiplier` | systemd Linux：启动 CWD（安装服务为 `/etc/miao`）；OpenWrt/非 systemd：`R`；Windows：应用数据目录 | systemd Linux / Windows：`P`；OpenWrt/非 systemd：该 Profile 的运行目录 |
| 节点 tag bindings | `config.yaml` 同目录的 `node-bindings.json` | 文件名为 `config.yaml` 时沿用同目录文件；其他文件名使用 `P/node-bindings.json` |

OpenWrt/非 systemd 的高频偏好与易变层仍在 tmpfs，系统重启后消失；稳定配置和节点绑定仍跟随配置文件。默认 Profile 的 CWD 偏好规则为兼容保留，命名 Profile 的归属不依赖启动 CWD。

桌面默认日志仍是进程级的应用数据目录 `miao.log`，保持托盘「打开日志」行为；它不是配置偏好。临时 Profile 的 Windows 日志使用自身临时目录，Unix 默认仍输出到终端。

## 旧文件与迁移

默认 Profile 的路径保持不变。命名 Profile **不自动继承旧的共享偏好、易变层或缓存**，因为无法判断这些内容属于哪份配置；旧文件也不会被删除。第一次使用新的隔离路径时按自身 YAML 默认值启动，订阅可能需要重新获取。

旧版非 `config.yaml` 配置的绑定文件形如 `travel.node-bindings.json`。新绑定文件不存在时，启动会将旧字节原子复制到该 Profile 的 `P/node-bindings.json`，保留既有 tag 记录；原文件不删除，新文件已存在则不覆盖。即使旧版本因相同文件 stem 共享了绑定文件，之后也分别写入独立目录，不再相互覆盖。

## SDK 与生命周期

- `spawn_server(RuntimeOptions)` 不读取宿主进程 argv。`config_path: None` 只执行默认文件发现；CLI 和桌面壳通过同一个参数解析器显式传入配置路径，保留 `--config PATH` / `--config=PATH`。
- `runtime_dir` 显式覆盖运行文件及默认偏好目录；所有平台的默认 `volatile.yaml` 也随它走。单独指定 `volatile_path` 优先级更高。Windows 的运行目录覆盖还隔离默认日志；显式 `log_path` 优先。
- 调用方提供的配置/运行目录不归 runtime 删除。临时 `--sub` 的目录所有权由启动包装交给 `AppState`；请求和后台任务持有 `Arc<AppState>` 时，目录不会被提前删除。启动失败会释放所有权。
- `ServerHandle::shutdown()` 等待服务和内核关闭；被取消的后台任务释放最后一个状态引用后才删除临时目录。仅 drop handle 会发起异步停服，也不会提前删除仍在使用的目录。强杀、断电、exec 升级不承诺析构清理。

**Profile 隔离不等于多实例支持**：TUN、Clash API 端口及部分进程级设施仍共享。不要因此在生产代理旁启动另一个真实内核。测试使用注入的临时环境、空配置/假内核和 localhost HTTP，不读取或改动生产运行目录。
