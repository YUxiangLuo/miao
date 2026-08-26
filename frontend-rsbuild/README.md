# Miao Web 控制面板

这是 Miao 唯一的正式前端工程。生产构建由 Rsbuild 输出到仓库根目录 `public/`，随后通过 Rust 的 `include_str!` / `include_bytes!` 嵌入 `miao-core`，因此最终仍保持单二进制分发。

## 技术栈

- Rsbuild 2
- React 19
- TypeScript 7（strict）
- Rstest
- Rslint

## 开发

```bash
bun install
bun run dev
```

开发服务器默认监听 `127.0.0.1`，并把 `/api`（包括 WebSocket）代理到 `http://127.0.0.1:6161`。后端使用其他端口时：

```bash
MIAO_API=http://127.0.0.1:7000 bun run dev
```

## API 类型

`src/types/api.ts` 由 Rust serde 模型生成，禁止手工编辑。修改 `crates/miao-core/src/models/*.rs` 后在仓库根目录运行：

```bash
./scripts/generate-api-types.sh
```

Rust 测试会校验生成文件是否最新。`src/types/clash.ts` 描述的是被反代的 Clash API，不属于 Miao Rust schema，仍按前端实际消费面维护。

## 校验与生产构建

```bash
bun run typecheck
bun run lint
bun run test
bun run build
```

也可以从仓库根目录运行 `./scripts/build-frontend.sh`，它会以冻结的 Bun lockfile 安装依赖并将生产产物写入 `public/`。
