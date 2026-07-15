import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  optimizeDeps: {
    include: ["react", "react-dom/client"],
  },
  server: {
    host: "0.0.0.0",
    port: 1420,
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
  },
  plugins: [react()],
});
