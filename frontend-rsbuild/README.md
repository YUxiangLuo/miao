# Miao Web 控制面板

React、TypeScript strict、Rsbuild，使用 Rslint/Rstest；版本见 [package.json](package.json) 与 lockfile。构建到根目录 `public/`，由 Rust 嵌入，修改后须重建前端和 Miao。

## 开发

在本目录执行：

```bash
bun install --frozen-lockfile
bun run dev
```

开发服务监听 `127.0.0.1`，将 `/api`（含 WebSocket）代理到 `http://127.0.0.1:6161`；更换后端用 `MIAO_API=http://127.0.0.1:7000 bun run dev`。写操作使用隔离后端或 mock，遵守[生产实例限制](../DEV_NOTES.md#arch-生产实例)。

## 检查与测试

```bash
bun run lint
bun run typecheck
bun run test
bun run build
```

`test:watch` 用于调试，`preview` 预览产物；根目录 `./dev.sh check frontend` 冻结安装并执行全部前端检查和构建。升级 TypeScript 至少跑 lint、typecheck、test。

- API/Clash mock 使用 [testFixtures.ts](src/testFixtures.ts) 工厂，仅覆盖差异字段。App 集成测试 mock `/api/status|subs|nodes|rules|version` 与 Clash 端点；[setupTests.ts](src/setupTests.ts) 提供深色 `matchMedia` 和内存 `localStorage`。
- 浏览器用 `agent-browser`，截图用 `agent-browser screenshot [--full] <path>`；`eval` 使用 IIFE。修改 React 受控输入须用原生 setter 并触发 input/change 事件。

## API 类型与请求

[api.ts](src/types/api.ts) 由 Rust serde models 生成，禁止手改。修改 `crates/miao-core/src/models/*.rs` 后在根目录运行 `./dev.sh types`，常规 Rust 测试会检查一致性。[clash.ts](src/types/clash.ts) 按实际消费的 Clash API 手工维护。

状态、代理与连接每 3 秒轮询；订阅、手动节点、规则按 `data_revision` 刷新，30 秒兜底。首次读取共用轮询器，请求必须有超时、取消和代次校验，旧响应不得覆盖新状态；清空/卸载取消测速批次，自动历史与手动测量按时间合并。

连接列表保留虚拟滚动，使用 `CONNECTION_ROW_HEIGHT` / `CONNECTION_ROW_GAP`；FLIP 只在顺序变化时测量可视行。规则活跃判断先建立连接签名索引，避免全量交叉扫描。

## 设计与静态资源

[tokens.css](src/styles/tokens.css) 是颜色、字号、几何、层级和动效的唯一样式来源，组件不硬编码颜色、尺寸、时长。仅断点、≤3px 光学校正、onboarding 门面、第三方品牌色可例外，须注释说明。

| 约定 | 用法 |
| --- | --- |
| 颜色 | 紫：交互/选中/上传；蓝：代理/下载；绿：直连；红：拦截；琥珀：警告。选中背景用 `--accent-tint` |
| 派生色 | alpha 面/描边用 `color-mix(in srgb, var(--token) N%, transparent)`，不跨语义复用 |
| 圆角 | `--r-sm` 控件、`--r-md` 内容、`--r-lg` 容器、`--r-xl` 浮层、`--r-pill` 胶囊；外层比内层大一级 |
| JS 常量 | 图标、logo、断点、列表几何使用 [tokens.ts](src/tokens.ts)，与 CSS 同步 |

主题显式 dark/light，默认深色、不跟随系统；仅 tokens 首段区分主题，`miao-theme`、`useTheme`、`index.html` 防闪烁脚本须一致。

PWA 资源在本目录 `public/`；新增资源须在 Rust [静态资源处理器](../crates/miao-core/src/handlers/static_assets.rs) 嵌入并在 [router.rs](../crates/miao-core/src/router.rs) 注册。service worker 仅导航 network-first 兜底，**不缓存 `/api`**。
