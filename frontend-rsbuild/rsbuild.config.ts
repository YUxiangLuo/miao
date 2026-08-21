import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

// 与现有 frontend/vite.config.ts 保持相同的本地后端入口。
// 后端端口有调整时：MIAO_API=http://localhost:7000 bun run dev
const miaoApiTarget = process.env.MIAO_API ?? 'http://localhost:6161';

// Docs: https://rsbuild.rs/config/
export default defineConfig({
  plugins: [pluginReact()],
  html: {
    title: 'miao',
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
