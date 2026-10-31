import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  ImportCandidatesDialog,
  uniqueProviderId,
} from "@/components/providers/ImportCandidatesDialog";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

describe("ImportCandidatesDialog (get_import_candidates)", () => {
  it("derives a free id from the hint", () => {
    expect(uniqueProviderId("relay", new Set())).toBe("relay");
    expect(uniqueProviderId("relay", new Set(["relay", "relay-2"]))).toBe(
      "relay-3",
    );
    expect(uniqueProviderId("  ", new Set())).toBe("imported");
  });

  it("imports the selected candidates and leaves already-listed ones unchecked", async () => {
    const api = mockInvoke({
      get_import_candidates: [
        {
          idHint: "relay",
          name: "Relay",
          settingsConfig: { config: 'model_provider = "relay"' },
          fingerprint: "fp-relay",
        },
        {
          idHint: "existing",
          name: "Existing",
          settingsConfig: { config: 'model_provider = "existing"' },
          fingerprint: "fp-existing",
        },
      ],
      add_provider: true,
    });
    const onOpenChange = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ImportCandidatesDialog
          open
          appId="codex"
          existingIds={["existing"]}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("Relay")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Relay" })).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "Existing" }),
    ).not.toBeChecked();
    expect(api.callsTo("get_import_candidates")[0]?.args).toEqual({
      app: "codex",
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Import selected (1)" }),
    );
    await waitFor(() => expect(api.callsTo("add_provider")).toHaveLength(1));
    const args = api.callsTo("add_provider")[0].args as {
      app: string;
      addToLive: boolean;
      provider: { id: string; name: string; settingsConfig: unknown };
    };
    expect(args.app).toBe("codex");
    expect(args.addToLive).toBe(false);
    expect(args.provider).toMatchObject({
      id: "relay",
      name: "Relay",
      settingsConfig: { config: 'model_provider = "relay"' },
    });
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });
});
