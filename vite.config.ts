/// <reference types="vitest/config" />
import { defineConfig } from "vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  base: "./",
  clearScreen: false,
  // Copied as they are. theme-boot.js runs before the first paint (a classic script; the CSP allows only 'self').
  // Under src/ so scripts/launch-user.ps1 sees a change and rebuilds. samples/ is served by the dev
  // server for the browser preview (?sample=1) and is not part of the build.
  publicDir: "src/public",
  server: {
    port: 1441,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1442 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    // theme.test.ts reads the token files as text (?raw); vitest skips CSS unless it is listed here
    css: { include: [/\/src\/shell\/theme\/[^/?]+\.css/] },
  },
}));
