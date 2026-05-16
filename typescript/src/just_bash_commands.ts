import { defineCommand } from "just-bash";
import type { Command, ExecResult } from "just-bash";

import { parseToolArgv, type ToolSpec } from "./rust_core.js";
import { normalizeStructuredArgValues } from "./tool_specs.js";

export interface JustBashCommandRegistration {
  providerName: string;
  commandName: string;
  backendToolName: string;
  helpToolName: string;
  command: Command;
}

export interface JustBashCommandSource {
  providerName: string;
  commandName: string;
  backendToolName: string;
  helpToolName: string;
  tool: ToolSpec;
  invoke(input: Record<string, unknown>): Promise<string>;
}

export function createJustBashCommandRegistrations(
  sources: JustBashCommandSource[],
): JustBashCommandRegistration[] {
  return sources.map((source) => ({
    providerName: source.providerName,
    commandName: source.commandName,
    backendToolName: source.backendToolName,
    helpToolName: source.helpToolName,
    command: defineCommand(source.commandName, async (args) => {
      try {
        const parsedInput = parseToolArgv(source.tool, args);
        const toolInput = normalizeStructuredArgValues(source.tool.inputSchema, parsedInput);
        return output(await source.invoke(toolInput));
      } catch (error) {
        return failure(error);
      }
    }),
  }));
}

export function installJustBashRegistrations(
  bash: unknown,
  registrations: JustBashCommandRegistration[],
): void {
  const host = bash as {
    customCommands?: Command[];
    registerCommand?: (command: Command) => void;
  };
  if (typeof host.registerCommand === "function") {
    for (const registration of registrations) host.registerCommand(registration.command);
  } else {
    host.customCommands = [
      ...(host.customCommands ?? []),
      ...registrations.map((registration) => registration.command),
    ];
  }
}

function output(stdout: string): ExecResult {
  return { stdout: `${stdout}\n`, stderr: "", exitCode: 0 };
}

function failure(error: unknown): ExecResult {
  const message = error instanceof Error ? error.message : String(error);
  return { stdout: "", stderr: `${message}\n`, exitCode: 1 };
}
