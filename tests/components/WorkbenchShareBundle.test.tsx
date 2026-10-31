import { fireEvent, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { ShareBundlePanel } from "@/components/workbench/ShareBundlePanel";
import { defaultSelection, pointerLabel } from "@/components/workbench/helpers";
import { server } from "../msw/server";
import {
  readJson as json,
  renderWithQuery as wrap,
} from "./workbenchTestUtils";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe("ShareBundlePanel (PRODUCT-22)", () => {
  it("exports with secrets stripped by default", async () => {
    const scopes: unknown[] = [];
    server.use(
      http.post(
        "http://tauri.local/export_share_bundle",
        async ({ request }) => {
          scopes.push(await json(request));
          return HttpResponse.json({
            path: null,
            providers: 0,
            mcpServers: 0,
            prompts: 0,
            secretsStripped: 0,
          });
        },
      ),
    );
    wrap(<ShareBundlePanel />);
    fireEvent.click(screen.getByRole("button", { name: /Export/ }));
    await waitFor(() => expect(scopes).toHaveLength(1));
    expect(scopes[0]).toMatchObject({
      scope: { providerApps: ["claude"], mcp: true, stripSecrets: true },
    });
  });

  it("previews a bundle and imports the selected items with typed keys", async () => {
    const applied: unknown[] = [];
    server.use(
      http.post("http://tauri.local/preview_share_bundle", () =>
        HttpResponse.json({
          createdAt: "",
          secretsStripped: true,
          items: [
            {
              key: "provider:claude:a",
              kind: "provider",
              app: "claude",
              id: "a",
              name: "New relay",
              status: "new",
              missingSecrets: ["/settingsConfig/env/ANTHROPIC_AUTH_TOKEN"],
            },
            {
              key: "provider:claude:b",
              kind: "provider",
              app: "claude",
              id: "b",
              name: "Same relay",
              status: "identical",
              missingSecrets: [],
            },
          ],
        }),
      ),
      http.post(
        "http://tauri.local/apply_share_bundle",
        async ({ request }) => {
          applied.push(await json(request));
          return HttpResponse.json({
            added: 1,
            updated: 0,
            skipped: 0,
            errors: [],
          });
        },
      ),
    );
    wrap(<ShareBundlePanel />);
    fireEvent.click(screen.getByRole("button", { name: /Choose file/ }));
    expect(await screen.findByText("New relay")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("ANTHROPIC_AUTH_TOKEN"), {
      target: { value: "sk-typed" },
    });
    fireEvent.click(screen.getByRole("button", { name: /Import 1 items/ }));
    await waitFor(() => expect(applied).toHaveLength(1));
    expect(applied[0]).toEqual({
      request: {
        keys: ["provider:claude:a"],
        overwrite: false,
        secrets: {
          "provider:claude:a": {
            "/settingsConfig/env/ANTHROPIC_AUTH_TOKEN": "sk-typed",
          },
        },
      },
    });
  });

  it("helpers", () => {
    expect(pointerLabel("/server/env/GITHUB~1TOKEN")).toBe("GITHUB/TOKEN");
    expect([
      ...defaultSelection({
        createdAt: "",
        secretsStripped: true,
        items: [
          {
            key: "k1",
            kind: "mcp",
            id: "1",
            name: "1",
            status: "conflict",
            missingSecrets: [],
          },
          {
            key: "k2",
            kind: "mcp",
            id: "2",
            name: "2",
            status: "identical",
            missingSecrets: [],
          },
        ],
      }),
    ]).toEqual(["k1"]);
  });
});
