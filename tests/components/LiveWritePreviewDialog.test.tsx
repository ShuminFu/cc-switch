import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  LiveWritePreviewDialog,
  supportsLivePreview,
} from "@/components/providers/LiveWritePreviewDialog";
import { ProviderActions } from "@/components/providers/ProviderActions";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

describe("LiveWritePreviewDialog (preview_live_write)", () => {
  it("is offered for Claude only", () => {
    expect(supportsLivePreview("claude")).toBe(true);
    expect(supportsLivePreview("codex")).toBe(false);
    expect(supportsLivePreview(undefined)).toBe(false);
  });

  it("renders the diff of each changed file and switches on confirm", async () => {
    const api = mockInvoke({
      preview_live_write: {
        app: "claude",
        providerId: "p1",
        supported: true,
        files: [
          {
            path: "/home/mock/.claude/settings.json",
            before: "{}",
            after: '{"env":{}}',
            changed: true,
            unifiedDiff: '-{}\n+{"env":{}}',
          },
          {
            path: "/home/mock/.claude/unchanged.json",
            before: "{}",
            after: "{}",
            changed: false,
            unifiedDiff: "",
          },
        ],
      },
    });
    const onConfirm = vi.fn();
    render(
      <LiveWritePreviewDialog
        open
        appId="claude"
        providerId="p1"
        providerName="Relay"
        onOpenChange={vi.fn()}
        onConfirm={onConfirm}
      />,
    );

    expect(
      await screen.findByText("/home/mock/.claude/settings.json"),
    ).toBeInTheDocument();
    expect(screen.queryByText("/home/mock/.claude/unchanged.json")).toBeNull();
    expect(screen.getByText('+{"env":{}}')).toBeInTheDocument();
    expect(api.callsTo("preview_live_write")[0]?.args).toEqual({
      app: "claude",
      providerId: "p1",
    });

    fireEvent.click(screen.getByRole("button", { name: "Enable" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("explains when the app has no planner", async () => {
    render(
      <LiveWritePreviewDialog
        open
        appId="claude"
        providerId="p1"
        providerName="Relay"
        onOpenChange={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(
        screen.getByText("A preview is not available for this app yet."),
      ).toBeInTheDocument(),
    );
  });

  it("shows the preview button in the card actions only when wired", () => {
    const onPreview = vi.fn();
    const { rerender } = rtlRender(
      <ProviderActions
        appId="claude"
        isCurrent={false}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
        onPreview={onPreview}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Preview changes" }));
    expect(onPreview).toHaveBeenCalled();

    rerender(
      <ProviderActions
        appId="codex"
        isCurrent={false}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Preview changes" })).toBeNull();
  });
});
