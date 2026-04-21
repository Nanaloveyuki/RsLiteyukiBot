import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";

const devHost = process.env.TAURI_DEV_HOST || "0.0.0.0";
const frontendRoot = fileURLToPath(new URL(".", import.meta.url));
const srcRoot = fileURLToPath(new URL("./src", import.meta.url));

export default defineConfig({
  root: frontendRoot,
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  resolve: {
    alias: {
      "@": srcRoot,
    },
  },
  server: {
    host: devHost,
    port: 1420,
    strictPort: true,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:14500",
        changeOrigin: false,
      },
      "/assets/bot.svg": {
        target: "http://127.0.0.1:14500",
        changeOrigin: false,
      },
      "/favicon.ico": {
        target: "http://127.0.0.1:14500",
        changeOrigin: false,
      },
    },
  },
  preview: {
    host: "0.0.0.0",
    port: 4173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
