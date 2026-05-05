import { expect, test } from "vitest";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { readFileSync } from "node:fs";

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = resolve(here, "..");
const srcRoot = resolve(pkgRoot, "src");

/**
 * The published `exports` map declares which sub-path entrypoints third-party consumers may import. These tests pin
 * the surface so we don't accidentally drop or rename one in a future refactor — and assert that the lightweight
 * entries (`./config`, `./errors`, `./types`) stay free of the heavy `fastmcp` runtime, which is the whole point of
 * having them as separate sub-paths.
 */

const pkgJson = JSON.parse(readFileSync(resolve(pkgRoot, "package.json"), "utf8")) as {
  exports: Record<string, { import: string; types: string }>;
};

test("package.json exports declares the lightweight sub-paths", () => {
  expect(Object.keys(pkgJson.exports).sort()).toEqual([
    ".",
    "./bash",
    "./config",
    "./errors",
    "./rust-core",
    "./types",
  ]);
});

test("each lightweight sub-path resolves to its own source file", () => {
  for (const sub of ["./config", "./errors", "./types"] as const) {
    const entry = pkgJson.exports[sub];
    expect(entry, `missing exports entry for ${sub}`).toBeDefined();
    expect(entry?.import).toMatch(/^\.\/dist\//);
    expect(entry?.types).toMatch(/^\.\/dist\//);
  }
});

/**
 * Walk the static (non-type-only) import graph rooted at the given source file and collect the set of bare-package
 * specifiers reachable from it. We only need to detect whether the heavy runtime entries (fastmcp, the modelcontext
 * SDK, etc.) appear — so a single-pass regex scanner is enough; we don't need a real ESM parser.
 */
function collectRuntimeImports(entryFile: string): Set<string> {
  const seen = new Set<string>();
  const externalSpecifiers = new Set<string>();
  const stack = [entryFile];
  while (stack.length > 0) {
    const file = stack.pop()!;
    if (seen.has(file)) continue;
    seen.add(file);
    let body: string;
    try {
      body = readFileSync(file, "utf8");
    } catch {
      continue;
    }
    const importRegex = /^[ \t]*import(?!\s+type\b)(?:[^"';]*?from)?\s*["']([^"']+)["']/gm;
    for (const match of body.matchAll(importRegex)) {
      const spec = match[1]!;
      if (spec.startsWith(".")) {
        const next = resolve(dirname(file), spec.replace(/\.js$/, ".ts"));
        stack.push(next);
        continue;
      }
      if (spec.startsWith("node:")) continue;
      externalSpecifiers.add(spec);
    }
  }
  return externalSpecifiers;
}

test.each([
  ["./config", resolve(srcRoot, "config.ts")],
  ["./errors", resolve(srcRoot, "errors.ts")],
  ["./types", resolve(srcRoot, "types.ts")],
])("sub-path %s does not pull fastmcp into the runtime graph", (subpath, srcFile) => {
  const externals = collectRuntimeImports(srcFile);
  // The heavy graph we explicitly want these sub-paths to avoid. If a refactor adds any of these as a runtime
  // dependency of config/errors/types, the test fails — and the fix is to either move the import behind a more
  // narrowly-scoped sub-path, or to revisit whether the lightweight contract still holds.
  const heavyForbidden = ["fastmcp", "@modelcontextprotocol/sdk", "@toon-format/toon", "commander"];
  for (const forbidden of heavyForbidden) {
    expect(externals, `${subpath} (${srcFile}) imported '${forbidden}'`).not.toContain(forbidden);
  }
});
