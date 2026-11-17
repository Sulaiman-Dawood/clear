import { defineConfig } from "vite";
import { resolve } from "node:path";

// Tauri expects a fixed dev port and serves the built files from dist/.
export default defineConfig({
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    outDir: "dist",
    rollupOptions: {
      input: {
        popup: resolve(import.meta.dirname, "popup.html"),
        settings: resolve(import.meta.dirname, "settings.html"),
      },
    },
  },
});
