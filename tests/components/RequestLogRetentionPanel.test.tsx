import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { RequestLogRetentionPanel } from "@/components/log-retention/RequestLogRetentionPanel";
import { server } from "../msw/server";

const toastSuccess = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: vi.fn(),
  },
}));

describe("RequestLogRetentionPanel (PRODUCT-31)", () => {
  it("shows the window and DB size, and deletes logs only after confirming", async () => {
    let pruned = 0;
    server.use(
      http.post("http://tauri.local/get_request_log_retention", () =>
        HttpResponse.json({
          retentionDays: 7,
          minDays: 1,
          maxDays: 3650,
          databaseBytes: 3 * 1024 * 1024,
        }),
      ),
      http.post("http://tauri.local/prune_request_logs_now", () => {
        pruned += 1;
        return HttpResponse.json(42);
      }),
    );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RequestLogRetentionPanel />
      </QueryClientProvider>,
    );

    expect(await screen.findByText(/3\.0 MB/)).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: "Request log retention" }),
    ).toHaveTextContent("7");

    fireEvent.click(
      screen.getByRole("button", { name: /Delete request logs now/ }),
    );
    expect(pruned).toBe(0);
    fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(pruned).toBe(1));
    await waitFor(() =>
      expect(toastSuccess).toHaveBeenCalledWith(expect.stringContaining("42")),
    );
  });
});

describe("RequestLogRetentionPanel stream-check retention", () => {
  it("shows and saves the connectivity-check log window", async () => {
    let saved: number | undefined;
    server.use(
      http.post("http://tauri.local/get_stream_check_retention", () =>
        HttpResponse.json({ retentionDays: 3 }),
      ),
      http.post(
        "http://tauri.local/set_stream_check_retention",
        async ({ request }) => {
          saved = ((await request.json()) as { days: number }).days;
          return HttpResponse.json({ retentionDays: saved });
        },
      ),
    );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RequestLogRetentionPanel />
      </QueryClientProvider>,
    );

    const trigger = await screen.findByRole("combobox", {
      name: "Connectivity check log retention",
    });
    await waitFor(() => expect(trigger).toHaveTextContent("3"));
    await waitFor(() => expect(trigger).not.toBeDisabled());
    fireEvent.click(trigger);
    fireEvent.click(
      await screen.findByRole("option", { name: "Keep forever" }),
    );
    await waitFor(() => expect(saved).toBe(0));
    await waitFor(() => expect(trigger).toHaveTextContent("Keep forever"));
  });
});
