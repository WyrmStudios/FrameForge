import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [react()],

  clearScreen: false,

  // Only scan src/ for dependencies — prevents Vite crawling src-tauri/target/
  optimizeDeps: {
    entries: ["src/**/*.{ts,tsx}"],
  },

  build: {
    // Use terser for more aggressive minification in production builds
    minify: "terser" as const,
    terserOptions: {
      compress: {
        drop_console: true,
        drop_debugger: true,
        passes: 2,
      },
      mangle: {
        // Mangle top-level names (functions, classes, variables)
        toplevel: true,
        // Mangle property names that start with _ (private convention)
        properties: {
          regex: /^_/,
        },
      },
      format: {
        // Remove all comments from the output
        comments: false,
      },
    },
  },

  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
