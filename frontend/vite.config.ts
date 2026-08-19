import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { viteSingleFile } from 'vite-plugin-singlefile'

// 后端 API 地址；后端端口在 config.yaml 里改过时，可用环境变量覆盖：
//   MIAO_API=http://localhost:7000 bun run dev
const miaoApiTarget = process.env.MIAO_API || 'http://localhost:6161'

export default defineConfig({
  plugins: [react(), viteSingleFile()],
  server: {
    proxy: {
      '/api': {
        target: miaoApiTarget,
        changeOrigin: true,
        ws: true,
      },
    },
  },
  build: {
    outDir: '../public',
    emptyOutDir: false,
    assetsInlineLimit: 100000000,
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        manualChunks: undefined,
        inlineDynamicImports: true,
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: './src/setupTests.js',
  },
})
