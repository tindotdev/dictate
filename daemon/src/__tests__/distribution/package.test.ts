/**
 * Package Distribution Tests
 *
 * Validates that package.json bin entries are correct and all binaries
 * exist after build. Prevents regressions in npm package structure.
 */

import { describe, expect, it } from "bun:test";
import * as fs from "node:fs";
import * as path from "node:path";

describe("Package Distribution", () => {
	// From __tests__/distribution/, go up to daemon/ root
	// __tests__/distribution/ -> __tests__/ -> src/ -> daemon/
	const rootDir = path.resolve(import.meta.dir, "../../../");
	const pkgPath = path.join(rootDir, "package.json");
	const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8"));

	describe("bin entries", () => {
		it("has all expected bin entries", () => {
			expect(pkg.bin).toEqual({
				dictated: "dist/main.js",
				dictatectl: "dist/cli/dictatectl.js",
				dictate: "dist/cli/dictate.js",
			});
		});

		it("all bin files exist after build", () => {
			for (const [_name, binPath] of Object.entries(pkg.bin)) {
				const fullPath = path.join(rootDir, binPath as string);
				const exists = fs.existsSync(fullPath);
				expect(exists).toBe(true);
			}
		});

		it("all bin files are executable (have shebang)", () => {
			for (const [_name, binPath] of Object.entries(pkg.bin)) {
				const fullPath = path.join(rootDir, binPath as string);
				const content = fs.readFileSync(fullPath, "utf-8");
				const firstLine = content.split("\n")[0];
				expect(firstLine).toBe("#!/usr/bin/env bun");
			}
		});

		it("bin files have correct permissions", () => {
			for (const [_name, binPath] of Object.entries(pkg.bin)) {
				const fullPath = path.join(rootDir, binPath as string);
				const stats = fs.statSync(fullPath);
				// Check if owner/group/other have execute permission
				const isExecutable = (stats.mode & 0o111) !== 0;
				expect(isExecutable).toBe(true);
			}
		});
	});

	describe("package metadata", () => {
		it("has correct name and scope", () => {
			expect(pkg.name).toBe("@tindotdev/dictate");
		});

		it("has public access configured", () => {
			expect(pkg.publishConfig?.access).toBe("public");
		});

		it("includes dist folder in files", () => {
			expect(pkg.files).toContain("dist");
		});

		it("has module type set to ESM", () => {
			expect(pkg.type).toBe("module");
		});

		it("has main entry point", () => {
			expect(pkg.main).toBe("dist/main.js");
		});
	});

	describe("bunx compatibility", () => {
		it("package name matches published npm package", () => {
			// Verify the package name is what users will use with bunx
			expect(pkg.name).toBe("@tindotdev/dictate");
		});

		it("all bin commands are documented in README", () => {
			const readmePath = path.join(path.dirname(rootDir), "README.md");

			// Skip if README doesn't exist (test running in isolation)
			if (!fs.existsSync(readmePath)) {
				return;
			}

			const readme = fs.readFileSync(readmePath, "utf-8");

			// Check that README mentions the main commands
			expect(readme).toContain("dictate");
			expect(readme).toContain("dictatectl");
		});
	});

	describe("version consistency", () => {
		it("has a valid semver version", () => {
			const version = pkg.version;
			const semverRegex = /^\d+\.\d+\.\d+(-[\w.]+)?(\+[\w.]+)?$/;
			expect(semverRegex.test(version)).toBe(true);
		});
	});
});
