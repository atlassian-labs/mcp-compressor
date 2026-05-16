import type { CompressorProxy, JustBashCommand } from "./native_client.js";
import type { ToolSpec } from "./rust_core.js";
import {
  createJustBashCommandRegistrations,
  installJustBashRegistrations,
  type JustBashCommandRegistration,
  type JustBashCommandSource,
} from "./just_bash_commands.js";

function commandToToolSpec(command: JustBashCommand): ToolSpec {
  return {
    name: command.backendToolName,
    description: command.description,
    inputSchema: command.inputSchema,
  };
}

export function installJustBashCommands(
  bash: unknown,
  proxy: CompressorProxy,
): JustBashCommandRegistration[] {
  const registrations = createJustBashCommands(proxy);
  installJustBashRegistrations(bash, registrations);
  return registrations;
}

export function createJustBashCommands(proxy: CompressorProxy): JustBashCommandRegistration[] {
  const rawNames = proxy.justBashProviders.flatMap((provider) =>
    provider.tools.map((tool) => tool.commandName),
  );
  const duplicateNames = new Set(
    rawNames.filter((name, index) => rawNames.indexOf(name) !== index),
  );
  const sources: JustBashCommandSource[] = [];
  for (const provider of proxy.justBashProviders) {
    for (const tool of provider.tools) {
      const commandName = duplicateNames.has(tool.commandName)
        ? `${provider.providerName}_${tool.commandName}`
        : tool.commandName;
      sources.push({
        providerName: provider.providerName,
        commandName,
        backendToolName: tool.backendToolName,
        helpToolName: provider.helpToolName,
        tool: commandToToolSpec(tool),
        invoke: (toolInput) =>
          proxy.invoke(tool.backendToolName, toolInput, {
            server: provider.providerName,
          }),
      });
    }
  }
  return createJustBashCommandRegistrations(sources);
}
