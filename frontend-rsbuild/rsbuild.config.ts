import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

// 与现有 frontend/vite.config.ts 保持相同的本地后端入口。
// 后端端口有调整时：MIAO_API=http://localhost:7000 bun run dev
const miaoApiTarget = process.env.MIAO_API ?? 'http://localhost:6161';

// Docs: https://rsbuild.rs/config/
export default defineConfig({
  plugins: [pluginReact()],
  source: {
    entry: {
      index: './src/main.tsx',
    },
  },
  html: {
    template: './index.html',
    // inlineScripts 内联后 defer 无效，脚本必须注入 body 末尾，
    // 否则在 <head> 解析阶段就执行，#root 尚不存在。
    inject: 'body',
  },
  output: {
    cleanDistPath: true,
    dataUriLimit: 100_000_000,
    distPath: {
      root: '../public',
    },
    inlineScripts: true,
    inlineStyles: true,
  },
  server: {
    proxy: {
      '/api': {
        target: miaoApiTarget,
        changeOrigin: true,
        ws: true,
      },
    },
  },
});
