import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "0.0.0.0",
    port: 1431,
    strictPort: true,
    allowedHosts: ["terminal.local"],
  },
  envPrefix: ["VITE_", "TAURI_"],
});
