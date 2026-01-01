import { defineConfig } from "tsup";

export default defineConfig({
	entry: ["src/main.ts", "src/cli/dictatectl.ts"],
	format: ["esm"],
	target: "node20",
	outDir: "dist",
	clean: true,
	sourcemap: true,
	dts: true,
});
