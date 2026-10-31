import { fireEvent, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { CustomToolsPanel } from "@/components/workbench/CustomToolsPanel";
import { server } from "../msw/server";
import {
  readJson as json,
  renderWithQuery as wrap,
} from "./workbenchTestUtils";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe("CustomToolsPanel (PRODUCT-43)", () => {
  it("lists descriptor tools and applies a profile without exposing its key", async () => {
    const activated: unknown[] = [];
    server.use(
      http.post("http://tauri.local/list_custom_tools", () =>
        HttpResponse.json({
          descriptorsDir: "/home/u/.cc-switch/custom-tools",
          descriptorErrors: [{ file: "bad.json", error: "invalid tool id" }],
          tools: [
            {
              descriptor: {
                id: "qwen-code",
                name: "Qwen Code",
                builtin: true,
                fields: [
                  {
                    id: "baseUrl",
                    label: "Base URL",
                    file: ".qwen/.env",
                    format: "env",
                    key: "OPENAI_BASE_URL",
                  },
                  {
                    id: "apiKey",
                    label: "API key",
                    file: ".qwen/.env",
                    format: "env",
                    key: "OPENAI_API_KEY",
                    secret: true,
                  },
                ],
              },
              installed: true,
              files: ["/home/u/.qwen/.env"],
              live: {},
              liveSecretsSet: [],
              profiles: [
                {
                  id: "p1",
                  name: "Relay",
                  values: { baseUrl: "https://relay.example/v1" },
                  secretsSet: ["apiKey"],
                },
              ],
              activeProfile: null,
            },
          ],
        }),
      ),
      http.post(
        "http://tauri.local/activate_custom_tool_profile",
        async ({ request }) => {
          activated.push(await json(request));
          return HttpResponse.json(1);
        },
      ),
    );
    wrap(<CustomToolsPanel />);
    expect(await screen.findByText("Qwen Code")).toBeInTheDocument();
    expect(screen.getByText(/bad\.json/)).toBeInTheDocument();
    expect(screen.getByText(/relay\.example/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Apply/ }));
    await waitFor(() =>
      expect(activated).toEqual([{ toolId: "qwen-code", profileId: "p1" }]),
    );
    // Editing shows the key as "saved", never its value.
    fireEvent.click(screen.getByRole("button", { name: "Edit profile" }));
    expect(screen.getByLabelText("API key")).toHaveValue("");
    expect(screen.getByLabelText("API key")).toHaveAttribute(
      "placeholder",
      expect.stringContaining("keep"),
    );
  });
});
