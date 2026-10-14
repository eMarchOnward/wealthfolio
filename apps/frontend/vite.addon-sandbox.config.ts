import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { defineConfig } from "vite";
import { verifyAddonSandboxRuntime } from "./scripts/verify-addon-sandbox-runtime.mjs";

const buildTarget = process.env.BUILD_TARGET || "tauri";

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    {
      name: "verify-addon-sandbox-runtime",
      async writeBundle() {
        await verifyAddonSandboxRuntime();
      },
    },
  ],
  publicDir: false,
  define: {
    __BUILD_TARGET__: JSON.stringify(buildTarget),
  },
  resolve: {
    alias: {
      "@wealthfolio/addon-sdk": path.resolve(__dirname, "../../packages/addon-sdk/src"),
      "@wealthfolio/ui": path.resolve(__dirname, "../../packages/ui/src"),
      "@/lib/utils": path.resolve(__dirname, "../../packages/ui/src/lib/utils"),
      "@": path.resolve(__dirname, "./src"),
    },
    extensions: [".js", ".ts", ".jsx", ".tsx", ".json"],
  },
  build: {
    assetsInlineLimit: 20 * 1024 * 1024,
    // The runtime is deliberately one inlined bundle; it cannot be code-split.
    chunkSizeWarningLimit: 4 * 1024,
    copyPublicDir: false,
    cssCodeSplit: false,
    emptyOutDir: !process.argv.includes("--watch"),
    minify: "esbuild",
    outDir: "public/__generated__",
    target: ["chrome107", "edge107", "firefox104", "safari16"],
    rollupOptions: {
      input: path.resolve(__dirname, "src/addons/iframe/addon-sandbox-entry.tsx"),
      onwarn(warning, warn) {
        // Rollup drops the "use client" directives in @wealthfolio/ui and then
        // fails to map that warning back to the source, once per file.
        if (warning.code === "MODULE_LEVEL_DIRECTIVE" || warning.code === "SOURCEMAP_ERROR") {
          return;
        }
        warn(warning);
      },
      output: {
        assetFileNames: (assetInfo) =>
          assetInfo.names.some((name) => name.endsWith(".css"))
            ? "addon-sandbox-runtime.css"
            : "[name][extname]",
        entryFileNames: "addon-sandbox-runtime.js",
        format: "es",
        inlineDynamicImports: true,
      },
    },
    sourcemap: false,
  },
});
