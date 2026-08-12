import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  server: {
    host: "0.0.0.0",
    port: 1420,
    strictPort: true,
    allowedHosts: ["terminal.local", "preview.local", ".localhost"],
    warmup: {
      clientFiles: ["./src/main.tsx", "./src/App.tsx", "./src/styles.css"],
    },
  },
  build: {
    outDir: "dist/client",
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return;
          if (id.includes("@tauri-apps")) return "desktop-runtime";
          if (id.includes("@phosphor-icons")) return "icons";
          if (id.includes("antd") || id.includes("@ant-design") || id.includes("rc-")) return "ui-vendor";
          if (id.includes("react") || id.includes("zustand") || id.includes("@tanstack")) return "react-vendor";
          return "vendor";
        },
      },
    },
  },
  optimizeDeps: {
    include: ["react", "react-dom/client"],
  },
  plugins: [react()],
});
