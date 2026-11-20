# HarborGate · 海港网关 Web 控制台

SolidJS 2.0、TypeScript strict、Rsbuild，使用 Rslint/Rstest；版本见 [package.json](package.json) 与 lockfile。构建到根目录 `public/`，由 Rust 嵌入，修改后须重建前端和网关二进制。

## 开发

在本目录执行：

```bash
bun install --frozen-lockfile
bun run dev
```

开发服务监听 `127.0.0.1`，将 `/api`（含 WebSocket）代理到 `http://127.0.0.1:6161`；更换后端用 `MIAO_API=http://127.0.0.1:7000 bun run dev`。写操作使用隔离后端或 mock，遵守[生产实例限制](../docs/development.md#arch-生产实例)。

## 检查与测试

```bash
bun run lint
bun run typecheck
bun run test
bun run build
```

`test:watch` 用于调试，`preview` 预览产物；根目录 `./dev.sh check frontend` 执行上面全部步骤。辅助脚本单独用 `./dev.sh check scripts` 检查。升级 TypeScript 至少跑 lint、typecheck、test。

- API/Clash mock 使用 [testFixtures.ts](src/testFixtures.ts) 工厂，仅覆盖差异字段。App 集成测试 mock `/api/status|subs|nodes|rules|version` 与 Clash 端点；[setupTests.ts](src/setupTests.ts) 提供深色 `matchMedia` 和内存 `localStorage`。
- 浏览器用 `agent-browser`，截图用 `agent-browser screenshot [--full] <path>`；`eval` 使用 IIFE。修改表单输入时应触发 `input` / `change` 事件以同步 Solid 信号。

## API 类型与请求

[api.ts](src/types/api.ts) 由 Rust serde models 生成，禁止手改。修改 `crates/miao-core/src/models/*.rs` 后在根目录运行 `./dev.sh types`，常规 Rust 测试会检查一致性。[clash.ts](src/types/clash.ts) 按实际消费的 Clash API 手工维护。

状态、代理与连接每 3 秒轮询；订阅、手动节点、规则按 `data_revision` 刷新，30 秒兜底。首次读取共用轮询器，请求必须有超时、取消和代次校验，旧响应不得覆盖新状态；清空/卸载取消测速批次，自动历史与手动测量按时间合并。

连接列表保留虚拟滚动，使用 `CONNECTION_ROW_HEIGHT` / `CONNECTION_ROW_GAP`；FLIP 只在顺序变化时测量可视行。规则活跃判断先建立连接签名索引，避免全量交叉扫描。

## 样式与静态资源

全局企业云网关主题变量位于 [theme.css](src/styles/theme.css)，导航与抽屉骨架位于 [shell.css](src/styles/shell.css)，通用表格/徽标/表单组件位于 [components.css](src/styles/components.css)，各业务视图排版位于 [views.css](src/styles/views.css)，少量供 JS 布局/动效计算的数值常量位于 [tokens.ts](src/tokens.ts)。组件与样式表可根据迭代需要自由调整色板、圆角、间距或局部覆写。

主题支持显式 `light` / `dark` 切换（键名 `miao-theme`）；如需改动默认主题或首屏背景色，顺手同步 [index.html](index.html) 的防闪烁脚本即可。

PWA 资源在本目录 `public/`；新增资源须在 Rust [静态资源处理器](../crates/miao-core/src/handlers/static_assets.rs) 嵌入并在 [router.rs](../crates/miao-core/src/router.rs) 注册。service worker 仅导航 network-first 兜底，**不缓存 `/api`**。
