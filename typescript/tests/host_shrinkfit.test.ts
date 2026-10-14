import { promisify } from "node:util";
import { execFile } from "node:child_process";
import { mkdtempSync, writeFileSync, chmodSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Bash } from "just-bash";
import { describe, expect, it, vi } from "vitest";
import {
  shrinkfitTools,
  compressTools,
  createExecutableToolBridge,
  transformToolsForCliMode,
  transformToolsForCodeMode,
  transformToolsForJustBash,
  type ExecutableTool,
} from "../src/index.js";

const exec = promisify(execFile);
const value = {
  items: Array.from({ length: 80 }, (_, id) => ({ id, text: "雪".repeat(100) })),
  count: 80,
};
const original = JSON.stringify(value);
function fixture() {
  const execute = vi.fn(async () => value);
  const tools: Record<string, ExecutableTool<unknown>> = {
    records: { name: "records", inputSchema: { type: "object" }, execute },
  };
  return { tools, execute };
}
function responseId(preview: string): string {
  expect(Array.from(preview).length).toBeLessThanOrEqual(1800);
  const parsed = JSON.parse(preview);
  expect(parsed.truncation).toBeDefined();
  expect(parsed.items).toBeDefined();
  const id = parsed.truncation.message.match(/response_id=([a-f0-9-]+)/)?.[1];
  expect(id).toBeTruthy();
  return id;
}

describe("host-owned shrinkfit recovery", () => {
  it("is opt-in and validates limits before execution", () => {
    const { tools, execute } = fixture();
    expect(shrinkfitTools(tools).tools).toBe(tools);
    for (const shrinkfit of [0, -1, 1.1, NaN, Infinity, 2 ** 32])
      expect(() => shrinkfitTools(tools, { shrinkfit })).toThrow("positive integer");
    expect(execute).not.toHaveBeenCalled();
  });
  it("recovers exact original without repeating writes and isolates sessions", async () => {
    const { tools, execute } = fixture();
    const a = shrinkfitTools(tools, { shrinkfit: 1800 });
    const b = shrinkfitTools(tools, { shrinkfit: 1800 });
    const id = responseId((await a.tools.records!.execute({})) as string);
    expect(await a.tools.compressor_recover_response!.execute({ response_id: id })).toBe(original);
    await expect(b.tools.compressor_recover_response!.execute({ response_id: id })).rejects.toThrow(
      "NOT called",
    );
    expect(execute).toHaveBeenCalledTimes(1);
    a.close();
    b.close();
    await expect(a.tools.compressor_recover_response!.execute({ response_id: id })).rejects.toThrow(
      "closed",
    );
  });
  it("does not shrink errors, multimodal results, or short responses", async () => {
    for (const response of [
      "short",
      { isError: true, content: [{ type: "text", text: original }] },
      { content: [{ type: "image", data: original, mimeType: "image/png" }] },
    ]) {
      const t = shrinkfitTools(
        { tool: { name: "tool", inputSchema: {}, execute: async () => response } },
        { shrinkfit: 1800 },
      );
      expect(await t.tools.tool!.execute({})).toEqual(response);
      t.close();
    }
  });
  it("expires cached originals without retrying upstream", async () => {
    const { tools, execute } = fixture();
    const t = shrinkfitTools(tools, { shrinkfit: 1800 });
    const id = responseId((await t.tools.records!.execute({})) as string);
    const now = Date.now();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now + 300_001);
    try {
      await expect(
        t.tools.compressor_recover_response!.execute({ response_id: id }),
      ).rejects.toThrow("expired");
    } finally {
      clock.mockRestore();
      t.close();
    }
    expect(execute).toHaveBeenCalledTimes(1);
  });
  it("preserves plaintext and arrays through cache recovery", async () => {
    for (const response of [
      "line of prose\n".repeat(1000),
      Array.from({ length: 400 }, (_, id) => ({ id, text: "long".repeat(40) })),
    ]) {
      const t = shrinkfitTools(
        { tool: { name: "tool", inputSchema: {}, execute: async () => response } },
        { shrinkfit: 1800 },
      );
      try {
        const preview = (await t.tools.tool!.execute({})) as string;
        expect(Array.from(preview).length).toBeLessThanOrEqual(1800);
        const id = preview.match(/response_id=([a-f0-9-]+)/)?.[1];
        expect(id).toBeTruthy();
        expect(await t.tools.compressor_recover_response!.execute({ response_id: id })).toBe(
          typeof response === "string" ? response : JSON.stringify(response),
        );
      } finally {
        t.close();
      }
    }
  });
  it("evicts oldest responses at the entry limit and never reexecutes for misses", async () => {
    const { tools, execute } = fixture();
    const t = shrinkfitTools(tools, { shrinkfit: 1800 });
    try {
      const first = responseId((await t.tools.records!.execute({})) as string);
      for (let i = 0; i < 128; i++) await t.tools.records!.execute({});
      await expect(
        t.tools.compressor_recover_response!.execute({ response_id: first }),
      ).rejects.toThrow("evicted");
      expect(execute).toHaveBeenCalledTimes(129);
    } finally {
      t.close();
    }
  });
  it("isolates concurrent responses and refuses unsafe tiny-budget retries", async () => {
    const { tools, execute } = fixture();
    const t = shrinkfitTools(tools, { shrinkfit: 1800 });
    try {
      const previews = await Promise.all(
        Array.from({ length: 12 }, () => t.tools.records!.execute({})),
      );
      const ids = previews.map((p) => responseId(p as string));
      expect(new Set(ids).size).toBe(12);
      for (const id of ids)
        expect(await t.tools.compressor_recover_response!.execute({ response_id: id })).toBe(
          original,
        );
      expect(execute).toHaveBeenCalledTimes(12);
    } finally {
      t.close();
    }
    const tiny = shrinkfitTools(tools, { shrinkfit: 1 });
    try {
      await expect(tiny.tools.records!.execute({})).rejects.toThrow("after upstream execution");
    } finally {
      tiny.close();
    }
    expect(execute).toHaveBeenCalledTimes(13);
  });
  it("does not overwrite an upstream recovery tool", () => {
    const { tools } = fixture();
    tools.compressor_recover_response = tools.records!;
    tools.compressorRecoverResponse2 = tools.records!;
    const t = shrinkfitTools(tools, { shrinkfit: 1800 });
    expect(t.tools.compressor_recover_response_3).toBeDefined();
    t.close();
  });
  it("compressed tools expose recovery through their existing wrapper, ahead of TOON", async () => {
    const { tools, execute } = fixture();
    const t = compressTools(tools, { shrinkfit: 1800, toonify: true });
    const id = responseId(
      (await t.invoke_tool!.execute({ tool_name: "records", tool_input: {} })) as string,
    );
    expect(
      await t.invoke_tool!.execute({
        tool_name: "compressor_recover_response",
        tool_input: { response_id: id },
      }),
    ).toBe(original);
    expect(execute).toHaveBeenCalledTimes(1);
  });
  it("HTTP bridge publishes and executes the recovery tool", async () => {
    const { tools, execute } = fixture();
    const t = await createExecutableToolBridge(tools, { serverName: "fixture", shrinkfit: 1800 });
    try {
      expect(t.server.tools.map((t) => t.name)).toContain("compressor_recover_response");
      const id = responseId((await t.invokeTool("records")) as string);
      expect(await t.invokeTool("compressor_recover_response", { response_id: id })).toBe(original);
      expect(execute).toHaveBeenCalledTimes(1);
    } finally {
      t.close();
    }
  });
  it("Just Bash recovers in memory, with generated help and without host files", async () => {
    const { tools, execute } = fixture();
    const bash = new Bash();
    const t = transformToolsForJustBash(tools, {
      serverName: "fixture",
      bash,
      shrinkfit: 1800,
      toonify: true,
    });
    try {
      const help = await bash.exec("fixture --help");
      expect(help.stdout).toContain("compressor-recover-response");
      expect(t.tools.fixture_help!.description).toContain("compressor-recover-response");
      const out = await bash.exec("fixture records");
      expect(out.exitCode).toBe(0);
      const id = responseId(out.stdout.trimEnd());
      const full = await bash.exec(`fixture compressor-recover-response --response-id ${id}`);
      expect(full.exitCode).toBe(0);
      expect(full.stdout.trimEnd()).toBe(original);
      expect(execute).toHaveBeenCalledTimes(1);
    } finally {
      t.close();
    }
  });
  it.each(["cli", "python", "typescript"] as const)(
    "generated %s invokes and recovers through the actual client",
    async (kind) => {
      const { tools, execute } = fixture();
      const dir = mkdtempSync(join(tmpdir(), "host-shrinkfit-"));
      const options = { serverName: "fixture", outputDir: dir, shrinkfit: 1800 };
      const t =
        kind === "cli"
          ? await transformToolsForCliMode(tools, options)
          : await transformToolsForCodeMode(tools, { ...options, language: kind });
      try {
        for (const [name, content] of Object.entries(t.files)) {
          writeFileSync(join(dir, name), content);
          chmodSync(join(dir, name), 0o755);
        }
        async function call(tool: string, id?: string) {
          if (kind === "cli")
            return (
              await exec("bash", [
                join(dir, "fixture"),
                tool.replaceAll("_", "-"),
                ...(id ? ["--response-id", id] : []),
              ])
            ).stdout.trimEnd();
          if (kind === "python")
            return (
              await exec(
                process.env.PYTHON ?? "python3",
                [
                  "-c",
                  `import fixture; print(fixture.${tool}(${id ? `response_id=${JSON.stringify(id)}` : ""}))`,
                ],
                { cwd: dir },
              )
            ).stdout.trimEnd();
          return (
            await exec(
              "bun",
              [
                "-e",
                `import * as f from './fixture.ts'; console.log(await f.${tool.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())}(${id ? JSON.stringify(id) : ""}));`,
              ],
              { cwd: dir },
            )
          ).stdout.trimEnd();
        }
        const id = responseId(await call("records"));
        expect(await call("compressor_recover_response", id)).toBe(original);
        expect(execute).toHaveBeenCalledTimes(1);
      } finally {
        t.close();
        rmSync(dir, { recursive: true, force: true });
      }
    },
  );
});
