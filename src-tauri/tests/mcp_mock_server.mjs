// Minimal MCP stdio server used by tests/mcp_integration.rs.
// Speaks newline-delimited JSON-RPC per the MCP stdio transport spec.
import readline from "node:readline";

const rl = readline.createInterface({ input: process.stdin });
const PROTOCOL_VERSION = "2024-11-05";

rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;
  let message;
  try {
    message = JSON.parse(trimmed);
  } catch {
    return;
  }
  const { id, method, params } = message;
  if (id === undefined || id === null) return; // notifications

  let result;
  switch (method) {
    case "initialize":
      result = {
        protocolVersion: params?.protocolVersion ?? PROTOCOL_VERSION,
        capabilities: { tools: {} },
        serverInfo: { name: "mock-mcp", version: "0.1.0" },
      };
      break;
    case "tools/list":
      result = {
        tools: [
          {
            name: "echo",
            description: "Echo the provided text back to the caller",
            inputSchema: {
              type: "object",
              properties: { text: { type: "string" } },
              required: ["text"],
            },
          },
          {
            name: "fail",
            description: "Always returns a tool error",
            inputSchema: { type: "object" },
          },
        ],
      };
      break;
    case "tools/call": {
      const tool = params?.name;
      if (tool === "echo") {
        const text = params?.arguments?.text ?? "";
        result = {
          content: [{ type: "text", text: `echo: ${text}` }],
          isError: false,
        };
      } else {
        result = {
          content: [{ type: "text", text: "mock failure" }],
          isError: true,
        };
      }
      break;
    }
    default:
      process.stdout.write(
        JSON.stringify({ jsonrpc: "2.0", id, error: { code: -32601, message: "Method not found" } }) + "\n"
      );
      return;
  }
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, result }) + "\n");
});
