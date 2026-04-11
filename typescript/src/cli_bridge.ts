import http from "node:http";

import type { Tool } from "@modelcontextprotocol/sdk/types.js";

import {
  formatToolHelp,
  formatTopLevelHelp,
  parseArgvToToolInput,
  toolNameToSubcommand,
} from "./cli_tools.js";
interface CliBridgeRuntime {
  invokeToolForCli(
    toolName: string,
    toolInput: Record<string, unknown> | undefined,
  ): Promise<string>;
  listUncompressedTools(): Promise<Tool[]>;
}

interface ExecBody {
  argv?: string[];
}

export class CliBridge {
  private cachedTools: Tool[] | null = null;
  private readonly cliName: string;
  private readonly runtime: CliBridgeRuntime;
  private server: http.Server | null = null;
  private toolBySubcommand = new Map<string, Tool>();

  constructor(runtime: CliBridgeRuntime, cliName: string) {
    this.runtime = runtime;
    this.cliName = cliName;
  }

  get url(): string {
    const address = this.server?.address();
    if (!address || typeof address === "string") {
      throw new Error("CLI bridge is not listening.");
    }
    return `http://127.0.0.1:${address.port}`;
  }

  async start(port = 0): Promise<number> {
    await this.ensureTools();
    this.server = http.createServer(async (request, response) => {
      try {
        await this.handleRequest(request, response);
      } catch (error) {
        response.statusCode = 500;
        response.setHeader("content-type", "text/plain; charset=utf-8");
        response.end(error instanceof Error ? error.message : String(error));
      }
    });

    await new Promise<void>((resolve, reject) => {
      this.server?.once("error", reject);
      this.server?.listen(port, "127.0.0.1", () => resolve());
    });

    const address = this.server.address();
    if (!address || typeof address === "string") {
      throw new Error("CLI bridge failed to bind a local port.");
    }
    return address.port;
  }

  async close(): Promise<void> {
    if (!this.server) {
      return;
    }
    const server = this.server;
    this.server = null;
    await new Promise<void>((resolve, reject) => {
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve();
      });
    });
  }

  private async ensureTools(): Promise<void> {
    if (this.cachedTools) {
      return;
    }
    const tools = await this.runtime.listUncompressedTools();
    this.cachedTools = tools;
    this.toolBySubcommand = new Map(tools.map((tool) => [toolNameToSubcommand(tool.name), tool]));
  }

  private async handleRequest(
    request: http.IncomingMessage,
    response: http.ServerResponse,
  ): Promise<void> {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");

    if (request.method === "GET" && url.pathname === "/health") {
      this.send(response, 200, "ok");
      return;
    }

    if (request.method === "GET" && url.pathname === "/help") {
      this.send(response, 200, formatTopLevelHelp(this.cliName, this.cachedTools ?? []));
      return;
    }

    const toolHelpMatch = /^\/tools\/([^/]+)\/help$/.exec(url.pathname);
    if (request.method === "GET" && toolHelpMatch) {
      const tool = this.toolBySubcommand.get(decodeURIComponent(toolHelpMatch[1]!));
      if (!tool) {
        this.send(
          response,
          404,
          `Unknown subcommand: ${decodeURIComponent(toolHelpMatch[1]!)}\n\n${formatTopLevelHelp(this.cliName, this.cachedTools ?? [])}`,
        );
        return;
      }
      this.send(response, 200, formatToolHelp(this.cliName, tool));
      return;
    }

    const toolInvokeMatch = /^\/tools\/([^/]+)$/.exec(url.pathname);
    if (request.method === "POST" && toolInvokeMatch) {
      const subcommand = decodeURIComponent(toolInvokeMatch[1]!);
      const tool = this.toolBySubcommand.get(subcommand);
      if (!tool) {
        this.send(
          response,
          404,
          `Unknown subcommand: ${subcommand}\n\n${formatTopLevelHelp(this.cliName, this.cachedTools ?? [])}`,
        );
        return;
      }

      const form = await this.readFormBody(request);
      const argv = form.getAll("argv").map(String);
      const result = await this.invokeToolFromArgv(tool, argv);
      this.send(response, result.statusCode, result.body);
      return;
    }

    if (request.method === "POST" && url.pathname === "/exec") {
      const body = (await this.readJsonBody(request)) as ExecBody;
      const result = await this.exec(body.argv ?? []);
      this.send(response, result.statusCode, result.body);
      return;
    }

    this.send(response, 404, "not found");
  }

  private send(response: http.ServerResponse, statusCode: number, body: string): void {
    response.statusCode = statusCode;
    response.setHeader("content-type", "text/plain; charset=utf-8");
    response.end(body);
  }

  private async readJsonBody(request: http.IncomingMessage): Promise<unknown> {
    const raw = await this.readRawBody(request);
    return raw ? JSON.parse(raw) : {};
  }

  private async readFormBody(request: http.IncomingMessage): Promise<URLSearchParams> {
    return new URLSearchParams(await this.readRawBody(request));
  }

  private async readRawBody(request: http.IncomingMessage): Promise<string> {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    }
    return Buffer.concat(chunks).toString("utf8");
  }

  private async exec(argv: string[]): Promise<{ statusCode: number; body: string }> {
    await this.ensureTools();

    if (argv.length === 0 || argv[0] === "--help" || argv[0] === "-h") {
      return { statusCode: 200, body: formatTopLevelHelp(this.cliName, this.cachedTools ?? []) };
    }

    const subcommand = argv[0]!;
    const tool = this.toolBySubcommand.get(subcommand);
    if (!tool) {
      return {
        statusCode: 400,
        body: `Unknown subcommand: ${subcommand}\n\n${formatTopLevelHelp(this.cliName, this.cachedTools ?? [])}`,
      };
    }

    const rest = argv.slice(1);
    if (rest.includes("--help") || rest.includes("-h")) {
      return { statusCode: 200, body: formatToolHelp(this.cliName, tool) };
    }

    return this.invokeToolFromArgv(tool, rest);
  }

  private async invokeToolFromArgv(
    tool: Tool,
    argv: string[],
  ): Promise<{ statusCode: number; body: string }> {
    try {
      const toolInput = parseArgvToToolInput(argv, tool);
      return {
        statusCode: 200,
        body: await this.runtime.invokeToolForCli(tool.name, toolInput),
      };
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return {
        statusCode: 400,
        body: `${message}\n\n${formatToolHelp(this.cliName, tool)}`,
      };
    }
  }
}
