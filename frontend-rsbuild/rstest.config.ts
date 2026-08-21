import { withRsbuildConfig } from '@rstest/adapter-rsbuild';
import { defineConfig } from '@rstest/core';

// Docs: https://rstest.rs/config/
export default defineConfig({
  extends: withRsbuildConfig(),
  exclude: ['tests/**'],
  setupFiles: ['./src/setupTests.ts'],
  testEnvironment: 'jsdom',
});
