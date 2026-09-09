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
    // 是纯粹的假信号。所以只认这两个目录 —— 而不是简单排除 artifacts/。
    //
    // `tests/` 必须显式列出：只写 src/** 的时候，tests/ipc-contract.test.ts 存在
    // 却从不执行，而它恰好是那种"没人跑就完全没有价值"的测试（校验前端 invoke 的
    // 命令名都在 lib.rs 里注册过，tsc 和 cargo 都管不到这道接缝）。
    include: ["src/**/*.{test,spec}.{ts,tsx}", "tests/**/*.{test,spec}.{ts,tsx}"],
  },
}));

