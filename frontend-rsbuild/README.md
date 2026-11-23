# Harbor Web 控制台

SolidJS 2.0、TypeScript strict、Rsbuild，搭配 Rslint / Rstest（依赖版本见 [package.json](package.json)）。构建产物输出至仓库根目录 `public/` 并由 Rust 嵌入，修改后须重编前端与后端二进制。

## 开发与检查

在本目录执行：

```bash
bun install --frozen-lockfile
bun run dev        # 监听 127.0.0.1，代理 /api（含 WebSocket）到 http://127.0.0.1:6161
bun run lint
bun run typecheck
bun run test       # 调试可用 bun run test:watch
bun run build
```

- 切换代理后端用 `HARBOR_API=http://127.0.0.1:7000 bun run dev`。写操作使用隔离后端或 mock，遵守[生产实例限制](../docs/development.md#arch-生产实例)。
- 根目录 `./dev.sh check frontend` 等价于安装依赖并执行上述全套检查与构建。
- 集成测试统一使用 [testFixtures.ts](src/testFixtures.ts) 工厂（仅覆写差异字段），mock `/api/status|subs|nodes|rules|terminals|access-ports|version` 与 Clash 端点；[setupTests.ts](src/setupTests.ts) 提供 `matchMedia` 与内存 `localStorage`。
- 浏览器检查用 `agent-browser`（截图 `agent-browser screenshot [--full] <path>`，`eval` 用 IIFE；修改表单输入须派发 `input`/`change` 以同步 Solid 信号）。

## API 契约与性能约束

- [api.ts](src/types/api.ts) 由 Rust serde models 生成，**禁止手改**。修改 `crates/harbor-core/src/models/*.rs` 后在根目录运行 `./dev.sh types`（常规 Rust 测试会校验一致性）。[clash.ts](src/types/clash.ts) 按实际消费字段手工维护。
- 状态、代理与连接每 3 秒轮询；订阅、手动节点、规则、终端与独立端口按 `data_revision` 触发刷新（30 秒兜底）。请求须具备超时、取消与代次校验，防止旧响应覆盖新状态；清空/卸载取消测速批次，自动历史与手动测速按时间合并。
- 连接列表使用虚拟滚动（`CONNECTION_ROW_HEIGHT` / `CONNECTION_ROW_GAP`），FLIP 仅在顺序变化时测量可视行；规则活跃判断先建连接签名索引，避免全量交叉扫描。

## 样式与 PWA

- 主题变量位于 [theme.css](src/styles/theme.css)（浅色暖纸 / 深色碳黑，键名 `harbor-theme`），侧边栏与抽屉框架在 [shell.css](src/styles/shell.css)，通用组件在 [components.css](src/styles/components.css)，各业务视图在 [views.css](src/styles/views.css)，JS 布局常量在 [tokens.ts](src/tokens.ts)。若调整默认主题或首屏底色，同步更新 [index.html](index.html) 防闪烁脚本。
- PWA 资源位于本目录 `public/`；新增静态资源须同步在 [static_assets.rs](../crates/harbor-core/src/handlers/static_assets.rs) 嵌入并在 [router.rs](../crates/harbor-core/src/router.rs) 注册。Service Worker 仅对导航请求做 network-first 兜底，**绝不缓存 `/api`**。
