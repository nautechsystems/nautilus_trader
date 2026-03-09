import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

function normalizeBasePath(rawValue: string | undefined, fallback: string): string {
  const trimmed = (rawValue || "").trim();
  if (!trimmed) {
    return fallback;
  }

  const prefixed = trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
  return prefixed.endsWith("/") ? prefixed : `${prefixed}/`;
}

export default defineConfig(({ command }) => ({
  base: command === "serve" ? "/" : normalizeBasePath(process.env.PULSE_UI_BASE_PATH, "/pulse/"),
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./vitest.setup.ts"],
  },
  server: {
    host: "127.0.0.1",
    port: 5174,
    strictPort: true,
    proxy: {
      "/api": {
        target: process.env.FLUXAPI_URL || "http://127.0.0.1:5022",
        changeOrigin: true,
      },
    },
  },
  preview: {
    host: "127.0.0.1",
    port: 4174,
    strictPort: true,
  },
}));
