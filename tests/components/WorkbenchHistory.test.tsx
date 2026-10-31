import { fireEvent, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { ConfigHistoryPanel } from "@/components/workbench/ConfigHistoryPanel";
import { AuditLogPanel } from "@/components/workbench/AuditLogPanel";
import { matchesAuditFilter } from "@/components/workbench/helpers";
import { server } from "../msw/server";
import {
  readJson as json,
  renderWithQuery as wrap,
} from "./workbenchTestUtils";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe("ConfigHistoryPanel (PRODUCT-21)", () => {
  it("shows the diff of a version and restores it after confirming", async () => {
    const restored: unknown[] = [];
    server.use(
      http.post("http://tauri.local/list_config_history", () =>
        HttpResponse.json([
          {
            entityType: "provider",
            app: "claude",
            entityId: "p1",
            name: "My relay",
            versions: 2,
            lastTs: 2,
            deleted: false,
          },
        ]),
      ),
      http.post("http://tauri.local/get_provider_history", () =>
        HttpResponse.json([
          {
            versionId: "2-0",
            ts: 2,
            action: "update",
            source: "ui",
            snapshot: {},
            changes: [
              {
                path: "/settingsConfig/env/ANTHROPIC_BASE_URL",
                before: "https://old",
                after: "https://new",
              },
            ],
          },
          {
            versionId: "1-0",
            ts: 1,
            action: "baseline",
            source: "system",
            snapshot: {},
            changes: [],
          },
        ]),
      ),
      http.post(
        "http://tauri.local/restore_provider_version",
        async ({ request }) => {
          restored.push(await json(request));
          return HttpResponse.json({ recreated: false, missingSecrets: [] });
        },
      ),
    );
    wrap(<ConfigHistoryPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /My relay/ }));
    expect(await screen.findByText("https://new")).toBeInTheDocument();
    expect(screen.getByText("https://old")).toBeInTheDocument();
    // Only older versions can be restored (the newest is the current state).
    const restoreButtons = screen.getAllByRole("button", {
      name: /Restore this version/,
    });
    expect(restoreButtons).toHaveLength(1);
    fireEvent.click(restoreButtons[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(restored).toEqual([{ app: "claude", id: "p1", versionId: "1-0" }]),
    );
  });
});

describe("AuditLogPanel (PRODUCT-25)", () => {
  it("filters events by action, actor or id", async () => {
    server.use(
      http.post("http://tauri.local/list_audit_events", () =>
        HttpResponse.json([
          {
            ts: 2,
            actor: "deeplink",
            action: "provider.create",
            entityType: "provider",
            entityId: "relay-1",
            app: "claude",
          },
          {
            ts: 1,
            actor: "ui",
            action: "provider.switch",
            entityType: "provider",
            entityId: "p2",
            app: "codex",
          },
        ]),
      ),
    );
    wrap(<AuditLogPanel />);
    expect(await screen.findByText("provider.create")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText(/Filter/), {
      target: { value: "deeplink" },
    });
    expect(screen.queryByText("provider.switch")).not.toBeInTheDocument();
    expect(
      matchesAuditFilter(
        {
          ts: 0,
          actor: "cli",
          action: "a",
          entityType: "provider",
          entityId: "X",
        },
        "x",
      ),
    ).toBe(true);
  });
});
