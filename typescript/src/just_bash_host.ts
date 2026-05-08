import { defineCommand } from "just-bash";
import type { Command, ExecResult } from "just-bash";

import type { CompressorProxy, JustBashCommand } from "./native_client.js";
import { parseToolArgv, type ToolSpec } from "./rust_core.js";
export interface JustBashCommandRegistration {
  providerName: string;
  commandName: string;
  backendToolName: string;
  helpToolName: string;
  command: Command;
}

function commandToToolSpec(command: JustBashCommand): ToolSpec {
  return {
    name: command.backendToolName,
    description: command.description,
    inputSchema: command.inputSchema,
  };
}

function output(text: string): ExecResult {
  return { stdout: text, stderr: "", exitCode: 0 };
}

function failure(error: unknown): ExecResult {
  const message = error instanceof Error ? error.message : String(error);
  return { stdout: "", stderr: `${message}\n`, exitCode: 1 };
}

export function installJustBashCommands(
  bash: unknown,
  proxy: CompressorProxy,
): JustBashCommandRegistration[] {
  const registrations = createJustBashCommands(proxy);
  const host = bash as {
    customCommands?: Command[];
    registerCommand?: (command: Command) => void;
  };
  if (typeof host.registerCommand === "function") {
    for (const registration of registrations) {
      host.registerCommand(registration.command);
    }
  } else {
    host.customCommands = [
      ...(host.customCommands ?? []),
      ...registrations.map((item) => item.command),
    ];
  }
  return registrations;
}

export function createJustBashCommands(proxy: CompressorProxy): JustBashCommandRegistration[] {
  const rawNames = proxy.justBashProviders.flatMap((provider) =>
    provider.tools.map((tool) => tool.commandName),
  );
  const duplicateNames = new Set(
    rawNames.filter((name, index) => rawNames.indexOf(name) !== index),
  );
  const registrations: JustBashCommandRegistration[] = [];
  for (const provider of proxy.justBashProviders) {
    for (const tool of provider.tools) {
      const spec = commandToToolSpec(tool);
      const registeredCommandName = duplicateNames.has(tool.commandName)
        ? `${provider.providerName}_${tool.commandName}`
        : tool.commandName;
      registrations.push({
        providerName: provider.providerName,
        commandName: registeredCommandName,
        backendToolName: tool.backendToolName,
        helpToolName: provider.helpToolName,
        command: defineCommand(registeredCommandName, async (args) => {
          try {
            const toolInput = parseToolArgv(spec, args);
            return output(
              await proxy.invoke(tool.backendToolName, toolInput, {
                server: provider.providerName,
              }),
            );
          } catch (error) {
            return failure(error);
          }
        }),
      });
    }
  }
  return registrations;
}
