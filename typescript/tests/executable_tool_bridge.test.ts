import { afterEach, describe, expect, it } from "vitest";

import {
  createExecutableToolBridge,
  type ExecutableToolBridge,
} from "../src/executable_tool_bridge.js";
import type { ExecutableTool } from "../src/index.js";

const bridges: ExecutableToolBridge[] = [];

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.close();
});

function fixtureTools(): Record<string, ExecutableTool<unknown>> {
  return {
    echo: {
      name: "echo",
      description: "Echo a message.",
      inputSchema: {
        type: "object",
        properties: { message: { type: "string", description: "Message to echo." } },
        required: ["message"],
      },
      execute: async (input: Record<string, unknown>) => ({
        result: `echo:${String(input.message ?? "")}`,
      }),
    },
  };
}

describe("createExecutableToolBridge", () => {
  it("creates a bridge with stable server metadata and invokeTool", async () => {
    const bridge = await createExecutableToolBridge(fixtureTools(), { serverName: "alpha" });
    bridges.push(bridge);

    expect(bridge.serverName).toBe("alpha");
    expect(bridge.server).toMatchObject({
      name: "alpha",
      bridgeUrl: bridge.bridgeUrl,
      tools: [
        {
          name: "echo",
          description: "Echo a message.",
        },
      ],
    });
    expect(bridge.bridgeUrl).toMatch(/^http:\/\/127\.0\.0\.1:\d+$/);
    expect(bridge.token).toHaveLength(36);

    await expect(
      fetch(`${bridge.bridgeUrl}/health`).then((response) => response.text()),
    ).resolves.toBe("ok");
    await expect(bridge.invokeTool("echo", { message: "hello" })).resolves.toBe("echo:hello");
  });

  it("uses the same /exec bearer bridge contract as generated clients", async () => {
    const bridge = await createExecutableToolBridge(fixtureTools(), { serverName: "alpha" });
    bridges.push(bridge);

    const response = await fetch(`${bridge.bridgeUrl}/exec`, {
      method: "POST",
      headers: {
        authorization: `Bearer ${bridge.token}`,
        "content-type": "application/json",
      },
      body: JSON.stringify({ tool: "echo", input: { message: "direct" } }),
    });

    await expect(response.json()).resolves.toEqual({ result: "echo:direct" });
  });
});
