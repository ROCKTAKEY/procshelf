import { defineConfig } from "vite";

export default defineConfig({
  build: {
    rolldownOptions: {
      output: { entryFileNames: "app.js", assetFileNames: "app.[ext]" },
    },
  },
});
