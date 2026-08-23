import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

// 本地开发时将控制面 API 代理到正在运行的 miao 后端。
// 后端端口有调整时：MIAO_API=http://localhost:7000 bun run dev
const miaoApiTarget = process.env.MIAO_API ?? 'http://localhost:6161';

// Docs: https://rsbuild.rs/config/
export default defineConfig(({ command }) => ({
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
    // dev 服务从内存出页面，清 dist 只会把嵌进二进制的构建产物抹掉（cargo 编译依赖它）；
    // 仅 build 时清理
    cleanDistPath: command === 'build',
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
}));
