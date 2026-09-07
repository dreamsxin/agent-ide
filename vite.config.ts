import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  // Tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  // prevent vite from obscuring rust errors
  clearScreen: false,
  test: {
    // artifacts/e2e/ 下躺着整个仓库的拷贝（含 *.test.tsx）。默认 include 会把那些
    // 历史快照也当成测试跑：测试数被放大，而且一旦报错指的是旧代码而不是当前代码，
    // 是纯粹的假信号。只认 src/ 下的测试。
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
}));

