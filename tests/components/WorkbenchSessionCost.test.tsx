import { screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { SessionCostBadge } from "@/components/workbench/SessionCostBadge";
import { formatUsd } from "@/components/workbench/api";
import { server } from "../msw/server";
import {
  readJson as json,
  renderWithQuery as wrap,
} from "./workbenchTestUtils";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe("SessionCostBadge (PRODUCT-14)", () => {
  it("shows the session's cost and renders nothing without logs", async () => {
    server.use(
      http.post("http://tauri.local/get_session_usage", async ({ request }) => {
        const { sessionId } = await json(request);
        return HttpResponse.json(
          sessionId === "s1"
            ? {
                requests: 3,
                inputTokens: 1500,
                outputTokens: 500,
                cacheReadTokens: 0,
                cacheCreationTokens: 0,
                totalCostUsd: 1.234,
              }
            : null,
        );
      }),
    );
    const { container } = wrap(
      <>
        <SessionCostBadge sessionId="s1" />
        <SessionCostBadge sessionId="s2" />
      </>,
    );
    expect(await screen.findByText(/\$1\.23 · 2\.0k/)).toBeInTheDocument();
    expect(container.querySelectorAll("span[aria-label]")).toHaveLength(1);
    expect(formatUsd(0.01234)).toBe("$0.0123");
  });
});
