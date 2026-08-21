# miao Rsbuild frontend

这是与现有 `frontend/` 并行的新前端工程，用于逐步迁移到 Rsbuild 和 TypeScript 7。

当前阶段不会替换正式构建入口：根目录的 `build.sh` 与
`scripts/build-frontend.sh` 仍然构建旧前端。完成页面迁移、单文件产物适配和回归测试后，
再统一切换生产构建。

## 技术栈

- Rsbuild 2
- React 19
- TypeScript 7
- Rstest
- Rslint

## 开发

```bash
bun install
bun run dev
```

开发服务器默认把 `/api`（包括 WebSocket）代理到 `http://localhost:6161`。
后端使用其他端口时：

```bash
MIAO_API=http://localhost:7000 bun run dev
```

## 校验

```bash
bun run typecheck
bun run lint
bun run test
bun run build
```
