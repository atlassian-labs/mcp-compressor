/**
 * just-bash integration — converts MCP backend tools to just-bash custom commands.
 *
 * Each backend server becomes a single parent command (e.g. `alpha`) whose subcommands map to
 * the server's MCP tools.  Argument parsing and tool invocation reuse the existing CLI bridge
 * infrastructure.
 */

import type { Tool } from "@modelcontextprotocol/sdk/types.js";
import { defineCommand } from "just-bash";
import type { Command } from "just-bash";

import {
  formatToolHelp,
  formatTopLevelHelp,
  parseArgvToToolInput,
  toolNameToSubcommand,
} from "./cli_tools.js";
import type { CompressorRuntime } from "./runtime.js";

/**
 * Create a single just-bash parent command for a CompressorRuntime's backend server.
 *
 * The command is named after the server (e.g. `alpha`) and dispatches subcommands that
 * correspond to individual MCP tools (e.g. `alpha alpha-add --a 1 --b 2`).
 */
export function createBashCommand(runtime: CompressorRuntime, tools: Tool[]): Command {
  const cliName = toolNameToSubcommand(runtime.serverName ?? "mcp");

  return defineCommand(cliName, async (args, _ctx) => {
    if (args.length === 0 || args[0] === "--help" || args[0] === "-h") {
      return {
        stdout: formatTopLevelHelp(cliName, tools),
        stderr: "",
        exitCode: 0,
      };
    }

    const subcommandName = args[0]!;
    const subcommandArgs = args.slice(1);

    // Find the matching tool
    const tool = tools.find((t) => toolNameToSubcommand(t.name) === subcommandName);
    if (!tool) {
      return {
        stdout: "",
        stderr: `${cliName}: unknown subcommand '${subcommandName}'\n\n${formatTopLevelHelp(cliName, tools)}`,
        exitCode: 1,
      };
    }

    if (subcommandArgs.includes("--help") || subcommandArgs.includes("-h")) {
      return {
        stdout: formatToolHelp(cliName, tool),
        stderr: "",
        exitCode: 0,
      };
    }

    try {
      const toolInput = subcommandArgs.length > 0 ? parseArgvToToolInput(subcommandArgs, tool) : {};
      const result = await runtime.invokeToolForCli(tool.name, toolInput);
      return { stdout: result, stderr: "", exitCode: 0 };
    } catch (error) {
      return {
        stdout: "",
        stderr: (error as Error).message,
        exitCode: 1,
      };
    }
  });
}

/**
 * Build the tool description for a bash tool that includes custom commands from MCP servers.
 *
 * Reuses the CLI-mode top-level help text for each server so the LLM can discover subcommands.
 */
export function buildBashToolDescription(
  serverCommands: Array<{
    serverName: string;
    command: Command;
    tools: Tool[];
  }>,
): string {
  const commandsHelp = serverCommands
    .map(({ serverName, tools }) => formatTopLevelHelp(toolNameToSubcommand(serverName), tools))
    .join("\n\n---\n\n");

  return BASH_TOOL_DESCRIPTION_TEMPLATE.replace("{{commands_help}}", commandsHelp);
}

const BASH_TOOL_DESCRIPTION_TEMPLATE = `\
Execute bash commands in a sandboxed environment (just-bash).

Supports standard Unix utilities (grep, cat, jq, sed, awk, sort, find, and many more). \
In addition, the following custom commands are installed in the bash environment:

{{commands_help}}

When possible, these commands will return TOON-formatted responses to minimize token usage.

Run '<command> --help' for per-command/subcommand usage and options.
Run '<command> <subcommand> --json \\'{"key":"value"}\\'' for raw JSON input.`;
