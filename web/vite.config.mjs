import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

import { configuredDevPort } from "./scripts/dev-port.mjs";

const devPort = configuredDevPort(process.env.MINE_MAIL_DEV_PORT);

export default defineConfig({
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          const normalizedId = id.replaceAll("\\", "/");
          if (
            normalizedId.includes("/node_modules/react/") ||
            normalizedId.includes("/node_modules/react-dom/") ||
            normalizedId.includes("/node_modules/scheduler/")
          ) {
            return "react-runtime";
          }
          if (normalizedId.includes("/node_modules/simple-icons/")) {
            return "brand-icons";
          }
          return undefined;
        },
      },
    },
  },
  optimizeDeps: {
    include: ["react", "react-dom/client"],
  },
  server: {
    host: "0.0.0.0",
    port: devPort,
    strictPort: true,
    allowedHosts: ["terminal.local"],
    watch: {
      // Rust locks Windows PDB files while Tauri is compiling. Watching the
      // native build tree makes chokidar terminate the dev server with EBUSY.
      ignored: ["**/src-tauri/target/**"],
    },
    warmup: {
      clientFiles: ["./src/main.jsx"],
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.js"],
  },
  plugins: [react()],
});
