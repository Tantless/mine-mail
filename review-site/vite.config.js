import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    rollupOptions: {
      input: {
        home: resolve(import.meta.dirname, "index.html"),
        privacy: resolve(import.meta.dirname, "privacy/index.html"),
        terms: resolve(import.meta.dirname, "terms/index.html"),
        support: resolve(import.meta.dirname, "support/index.html"),
        deletion: resolve(import.meta.dirname, "data-deletion/index.html"),
      },
    },
  },
});

