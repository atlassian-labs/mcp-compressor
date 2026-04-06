export class ToolNotFoundError extends Error {
  constructor(toolName: string, availableTools: string[]) {
    super(`Tool not found: ${toolName}. Available tools: ${availableTools.sort().join(', ')}`);
    this.name = 'ToolNotFoundError';
  }
}

export class InvalidConfigurationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'InvalidConfigurationError';
  }
}
