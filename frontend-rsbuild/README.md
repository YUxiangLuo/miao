# Miao Web 控制面板

唯一的正式前端：React、TypeScript strict、Rsbuild，使用 Rslint 和 Rstest。版本以 [package.json](package.json) 和 lockfile 为准。构建输出到仓库根目录 `public/`，由 Rust 嵌入；前端修改后须重建页面和 Miao 才会进入成品。

## 开发

以下命令在本目录执行：

```bash
bun install --frozen-lockfile
bun run dev
```

开发服务监听 `127.0.0.1`，将 `/api`（含 WebSocket）代理到 `http://127.0.0.1:6161`。使用其他后端时设置 `MIAO_API=http://127.0.0.1:7000 bun run dev`。涉及写操作的调试使用隔离后端或 mock；本机生产代理的限制见[开发指南](../DEV_NOTES.md#arch-生产实例)。

## 检查与测试

```bash
bun run lint
bun run typecheck
bun run test
bun run build
```

根目录的 `./scripts/build-frontend.sh` 会冻结安装依赖并构建。升级 TypeScript 至少运行 lint、typecheck、test；`bun run test:watch` 用于调试，`bun run preview` 预览构建结果。

- API/Clash mock 使用 [`src/testFixtures.ts`](src/testFixtures.ts) 的工厂，只覆盖差异字段；不要手写完整响应对象。
- App 集成测试 mock `/api/status|subs|nodes|rules|version` 和 Clash 端点；[`setupTests.ts`](src/setupTests.ts) 统一提供深色 `matchMedia` 与内存 `localStorage`，避免运行时内建对象干扰 jsdom。
- 浏览器检查使用 `agent-browser`，截图用 `agent-browser screenshot [--full] <path>`。`eval` 中用 IIFE 避免跨调用变量冲突；修改 React 受控输入须调用原生 setter 并触发 input/change 事件。

## API 类型与请求

[`src/types/api.ts`](src/types/api.ts) 由 Rust serde models 生成，禁止手改。修改 `crates/miao-core/src/models/*.rs` 后在根目录运行 `./scripts/generate-api-types.sh`；Rust 测试检查生成结果。[`src/types/clash.ts`](src/types/clash.ts) 按实际消费的 Clash API 维护，不属于该 schema。

状态、代理和连接每 3 秒轮询；订阅、手动节点和规则按 `data_revision` 刷新，并每 30 秒兜底。首次读取共用轮询器，请求须有超时、取消和代次校验，晚到响应不能覆盖新状态。清空或卸载时取消测速批次；自动历史与手动测量按时间合并。

连接列表保留虚拟滚动，行高/间距使用 `CONNECTION_ROW_HEIGHT` / `CONNECTION_ROW_GAP`；FLIP 只测量可视行，顺序变化才执行。规则活跃判断先为连接建立签名索引，避免规则与连接的全量笛卡尔扫描。

## 设计与静态资源

[`src/styles/tokens.css`](src/styles/tokens.css) 是样式 token 的唯一来源：主题色、派生色、音阶、动效、层级、透明度、描边、控件几何、组件几何。组件禁止硬编码颜色、尺寸、时长；仅媒体查询断点、≤3px 光学校正、onboarding 门面和第三方品牌色可例外，并须注释说明。

| 约定 | 用法 |
| --- | --- |
| 颜色 | 紫色表示交互/选中/上传，蓝色表示代理出口/下载，绿直连、红拦截、琥珀警告；选中背景用 `--accent-tint`，不跨语义复用 |
| 派生色 | alpha 面与描边用 `color-mix(in srgb, var(--token) N%, transparent)` |
| 圆角 | `--r-sm` 控件、`--r-md` 内容块、`--r-lg` 容器、`--r-xl` 浮层、`--r-pill` 胶囊；嵌套外层比内层大一级 |
| JS 常量 | 图标、logo、断点、列表几何使用 [`src/tokens.ts`](src/tokens.ts)，与 CSS 对应项同步 |

主题只有显式 dark/light，默认深色，不跟随系统；仅 tokens 第一段区分主题，其余段共用。`miao-theme`、`useTheme` 和 `index.html` 的防闪烁脚本保持一致。

PWA 的 manifest、service worker、图标放在本目录 `public/`。新增资源还须在 Rust [`router.rs`](../crates/miao-core/src/router.rs) 注册；service worker 仅为导航做 network-first 兜底，**不缓存 `/api`**。
