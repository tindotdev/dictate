import { defineConfig } from "tsup";

export default defineConfig({
	entry: ["src/main.ts", "src/cli/dictatectl.ts"],
	format: ["esm"],
	target: "node20",
	outDir: "dist",
	clean: true,
	sourcemap: true,
	dts: true,
	// Add shebang to output files for npm bin executables
	banner: {
		js: "#!/usr/bin/env bun",
	},
});
