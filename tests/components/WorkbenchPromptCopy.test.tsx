import { fireEvent, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import { PromptCopyDialog } from "@/components/workbench/PromptCopyDialog";
import { server } from "../msw/server";
import {
  readJson as json,
  renderWithQuery as wrap,
} from "./workbenchTestUtils";

const toastSuccess = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: vi.fn(),
  },
}));

describe("PromptCopyDialog (PRODUCT-59)", () => {
  it("copies a prompt to the chosen apps and activates it there", async () => {
    const calls: unknown[] = [];
    server.use(
      http.post(
        "http://tauri.local/copy_prompt_to_apps",
        async ({ request }) => {
          calls.push(await json(request));
          return HttpResponse.json([
            { app: "codex", status: "created", activated: true },
            { app: "gemini", status: "created", activated: true },
          ]);
        },
      ),
    );
    const onClose = vi.fn();
    wrap(
      <PromptCopyDialog
        appId="claude"
        promptId="rules"
        promptName="Team rules"
        onClose={onClose}
      />,
    );
    // The source app and Claude Desktop (shares Claude's prompts) are not
    // offered as targets.
    expect(
      screen.queryByRole("checkbox", { name: "Claude" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Codex" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Gemini" }));
    fireEvent.click(screen.getByRole("button", { name: /Copy/ }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toEqual({
      app: "claude",
      promptId: "rules",
      targetApps: ["codex", "gemini"],
      activate: true,
    });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(toastSuccess).toHaveBeenCalledWith(expect.stringContaining("2"));
  });
});
