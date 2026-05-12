export interface ExecutableTool {
  name: string;
  description?: string;
  inputSchema: Record<string, unknown>;
  execute(input?: Record<string, unknown>): Promise<string>;
}

export interface AISDKToolFactory<TTool = unknown> {
  (options: {
    description?: string;
    inputSchema: Record<string, unknown>;
    execute: (input: Record<string, unknown>) => Promise<string>;
  }): TTool;
}

export function toAISDKTools<TTool = unknown>(
  tools: Record<string, ExecutableTool>,
  options: { tool?: AISDKToolFactory<TTool> } = {},
): Record<string, TTool | Omit<ExecutableTool, "name">> {
  const result: Record<string, TTool | Omit<ExecutableTool, "name">> = {};
  for (const [name, executable] of Object.entries(tools)) {
    const definition = {
      description: executable.description,
      inputSchema: executable.inputSchema,
      execute: (input: Record<string, unknown>) => executable.execute(input),
    };
    result[name] = options.tool ? options.tool(definition) : definition;
  }
  return result;
}

export function toMastraTools(
  tools: Record<string, ExecutableTool>,
): Record<string, Omit<ExecutableTool, "name">> {
  const result: Record<string, Omit<ExecutableTool, "name">> = {};
  for (const [name, executable] of Object.entries(tools)) {
    result[name] = {
      description: executable.description,
      inputSchema: executable.inputSchema,
      execute: (input: Record<string, unknown> = {}) => executable.execute(input),
    };
  }
  return result;
}
