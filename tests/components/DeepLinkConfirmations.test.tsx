import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import { McpConfirmation } from "@/components/deeplink/McpConfirmation";
import { PromptConfirmation } from "@/components/deeplink/PromptConfirmation";

// Base64 decoding lives in the backend (deeplink::DeepLinkDecoded); these
// dialogs must display the backend-decoded text and never re-decode it.

describe("PromptConfirmation", () => {
  it("shows backend-decoded content, not the raw base64", () => {
    const request: DeepLinkImportRequest = {
      version: "v1",
      resource: "prompt",
      app: "claude",
      name: "p",
      content: "5L2g5aW9",
      decoded: { content: "# 你好 🚀" },
    };
    render(<PromptConfirmation request={request} />);
    expect(screen.getByText("# 你好 🚀")).toBeInTheDocument();
    expect(screen.queryByText("5L2g5aW9")).not.toBeInTheDocument();
  });

  it("falls back to the raw parameter when the backend could not decode it", () => {
    render(
      <PromptConfirmation
        request={{
          version: "v1",
          resource: "prompt",
          app: "claude",
          content: "not base64!!",
        }}
      />,
    );
    expect(screen.getByText("not base64!!")).toBeInTheDocument();
  });
});

describe("McpConfirmation", () => {
  it("lists servers from the backend-decoded config", () => {
    render(
      <McpConfirmation
        request={{
          version: "v1",
          resource: "mcp",
          apps: "claude,codex",
          // URL-safe base64 the old frontend decoder could not read
          config: "eyJtY3BTZXJ2ZXJzIjp7fX0",
          decoded: {
            config: JSON.stringify({
              mcpServers: {
                fetch: { command: "uvx" },
                remote: { type: "http", url: "https://x.dev/mcp" },
              },
            }),
          },
        }}
      />,
    );
    expect(screen.getByText("fetch")).toBeInTheDocument();
    expect(screen.getByText(/Command: uvx/)).toBeInTheDocument();
    expect(screen.getByText(/URL: https:\/\/x\.dev\/mcp/)).toBeInTheDocument();
  });

  it("renders no servers when nothing was decoded", () => {
    render(
      <McpConfirmation
        request={{
          version: "v1",
          resource: "mcp",
          apps: "claude",
          config: "garbage",
        }}
      />,
    );
    expect(screen.queryByText(/Command:/)).not.toBeInTheDocument();
  });
});
