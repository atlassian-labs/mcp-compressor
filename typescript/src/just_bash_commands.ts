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
  const grouped = new Map<string, JustBashCommandSource[]>();
  for (const source of sources) {
    grouped.set(source.providerName, [...(grouped.get(source.providerName) ?? []), source]);
  }

  return [...grouped.entries()].map(([providerName, providerSources]) => {
    const bySubcommand = new Map(
      providerSources.flatMap((source) => [
        [source.commandName, source] as const,
        [source.backendToolName.replaceAll("_", "-"), source] as const,
        [source.backendToolName, source] as const,
      ]),
    );
    const first = providerSources[0];
    return {
      providerName,
      commandName: providerName,
      backendToolName: providerName,
      helpToolName: first?.helpToolName ?? `${providerName}_help`,
      command: defineCommand(providerName, async (args) => {
        try {
          const [subcommand, ...toolArgs] = args;
          if (subcommand === undefined || subcommand === "--help" || subcommand === "-h") {
            return output(dispatcherHelp(providerName, providerSources));
          }
          const source = bySubcommand.get(subcommand);
          if (source === undefined) {
            throw new Error(`Unknown ${providerName} subcommand: ${subcommand}`);
          }
          const parsedInput = parseToolArgv(source.tool, toolArgs);
          const toolInput = normalizeStructuredArgValues(source.tool.inputSchema, parsedInput);
          return output(await source.invoke(toolInput));
        } catch (error) {
          return failure(error);
        }
      }),
    };
  });
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

function dispatcherHelp(providerName: string, sources: JustBashCommandSource[]): string {
  const maxNameLength = Math.max(
    ...sources.map((source) => source.backendToolName.replaceAll("_", "-").length),
    0,
  );
  return [
    `${providerName} - the ${providerName} toolset`,
    "",
    "USAGE:",
    `  ${providerName} <subcommand> [options]`,
    "",
    "SUBCOMMANDS:",
    ...sources.map((source) => {
      const description = (source.tool.description ?? "").replace(/\s+/g, " ").trim();
      const subcommand = source.backendToolName.replaceAll("_", "-");
      return `  ${subcommand.padEnd(maxNameLength + 2)}${description}`.trimEnd();
    }),
  ].join("\n");
}

function output(stdout: string): ExecResult {
  return { stdout: `${stdout}\n`, stderr: "", exitCode: 0 };
}

function failure(error: unknown): ExecResult {
  const message = error instanceof Error ? error.message : String(error);
  return { stdout: "", stderr: `${message}\n`, exitCode: 1 };
}
