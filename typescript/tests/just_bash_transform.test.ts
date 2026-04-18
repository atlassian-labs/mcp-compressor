/** Tests for the just-bash AST transformer that injects ``MCP_TOONIFY`` hints. */

import { test, expect } from "vitest";
import { Bash, defineCommand } from "just-bash";
import type { CommandContext, ExecResult } from "just-bash";

import {
  MCP_TOONIFY_ENV_VAR,
  installPipingHintPlugin,
  resolveToonifyFromEnv,
} from "../src/just_bash_transform.js";

function makeCapturingCommand(name: string) {
  return defineCommand(name, async (_args: string[], ctx: CommandContext): Promise<ExecResult> => {
    const env = ctx.env;
    let value: string | undefined;
    if (env instanceof Map) {
      value = env.get(MCP_TOONIFY_ENV_VAR);
    } else if (env && typeof env === "object") {
      value = (env as Record<string, string>)[MCP_TOONIFY_ENV_VAR];
    }
    return { stdout: `toon=${value ?? "<unset>"}`, stderr: "", exitCode: 0 };
  });
}

function makeBash(commandNames: string[]): Bash {
  const bash = new Bash({ customCommands: commandNames.map(makeCapturingCommand) });
  installPipingHintPlugin(bash, commandNames);
  return bash;
}

test("unpiped wrapper command receives MCP_TOONIFY=true", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("alpha foo");
  expect(result.exitCode).toBe(0);
  expect(result.stdout.trim()).toBe("toon=true");
});

test("first command in pipeline receives MCP_TOONIFY=false", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("alpha foo | wc -c");
  expect(result.exitCode).toBe(0);
  // Byte-count of "toon=false" (10) or "toon=false\n" (11) depending on shell.
  expect(["10", "11"]).toContain(result.stdout.trim());
});

test("last command in pipeline receives MCP_TOONIFY=true", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("echo hi | alpha foo");
  expect(result.exitCode).toBe(0);
  expect(result.stdout.trim()).toBe("toon=true");
});

test("output redirection `> file` counts as piped", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("alpha foo > /tmp/out.json && cat /tmp/out.json");
  expect(result.exitCode).toBe(0);
  expect(result.stdout).toContain("toon=false");
});

test("append redirection `>> file` counts as piped", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("alpha foo >> /tmp/out2.json && cat /tmp/out2.json");
  expect(result.exitCode).toBe(0);
  expect(result.stdout).toContain("toon=false");
});

test("stderr redirection `2>&1` does not count as piped", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("alpha foo 2>&1");
  expect(result.exitCode).toBe(0);
  expect(result.stdout.trim()).toBe("toon=true");
});

test("logical chain `cmd && other` does not count as piped", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("alpha foo && echo done");
  expect(result.exitCode).toBe(0);
  expect(result.stdout).toContain("toon=true");
  expect(result.stdout).toContain("done");
});

test("explicit user MCP_TOONIFY assignment is preserved", async () => {
  const bash = makeBash(["alpha"]);
  // User-set value wins over auto-injection even when piped.
  const result = await bash.exec("MCP_TOONIFY=true alpha foo | wc -c");
  expect(result.exitCode).toBe(0);
  expect(["9", "10"]).toContain(result.stdout.trim());
});

test("multiple custom commands in a pipeline are each annotated", async () => {
  const bash = makeBash(["alpha", "beta"]);
  // alpha is piped (toon=false), beta is last (toon=true); we only see beta.
  const result = await bash.exec("alpha foo | beta bar");
  expect(result.exitCode).toBe(0);
  expect(result.stdout.trim()).toBe("toon=true");
});

test("non-custom commands are not annotated", async () => {
  const bash = makeBash(["alpha"]);
  const result = await bash.exec("echo hello | wc -c");
  expect(result.exitCode).toBe(0);
  expect(result.stdout.trim()).toBe("6"); // "hello\n"
});

test("dynamic command name (via parameter expansion) is left alone", async () => {
  const bash = makeBash(["alpha"]);
  // Dynamic names (param expansion) must be skipped without crashing.
  const result = await bash.exec("cmd=alpha; $cmd foo");
  expect(result.exitCode).toBe(0);
});

// ---- resolveToonifyFromEnv -------------------------------------------------

test("resolveToonifyFromEnv returns default when env is undefined", () => {
  expect(resolveToonifyFromEnv(undefined, true)).toBe(true);
  expect(resolveToonifyFromEnv(undefined, false)).toBe(false);
});

test("resolveToonifyFromEnv reads from a Map", () => {
  const env = new Map<string, string>([[MCP_TOONIFY_ENV_VAR, "true"]]);
  expect(resolveToonifyFromEnv(env, false)).toBe(true);
});

test("resolveToonifyFromEnv reads from a plain object", () => {
  expect(resolveToonifyFromEnv({ [MCP_TOONIFY_ENV_VAR]: "false" }, true)).toBe(false);
});

test("resolveToonifyFromEnv returns default for unrecognized values", () => {
  expect(resolveToonifyFromEnv({ [MCP_TOONIFY_ENV_VAR]: "maybe" }, true)).toBe(true);
});
