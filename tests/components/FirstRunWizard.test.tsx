import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import type { Settings } from "@/types";
import { server } from "../msw/server";
import { getSettings, setSettings } from "../msw/state";
import {
  FirstRunWizard,
  openFirstRunWizard,
  shouldAutoOpenFirstRun,
} from "@/components/onboarding";
import {
  DEFAULT_VISIBLE_APPS,
  detectedToolNames,
  initialSelection,
} from "@/components/onboarding/onboardingModel";

const addProviderMock = vi.hoisted(() => vi.fn());

vi.mock("@/hooks/useProviderActions", () => ({
  useProviderActions: () => ({ addProvider: addProviderMock }),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, appId, onSubmit, onOpenChange }: any) =>
    open ? (
      <div data-testid="add-provider-dialog" data-app={appId}>
        <button type="button" onClick={() => onSubmit({ name: "x" })}>
          submit-provider
        </button>
        <button type="button" onClick={() => onOpenChange(false)}>
          close-add
        </button>
      </div>
    ) : null,
}));

const TAURI = "http://tauri.local";

const mockToolVersions = (installed: string[]) =>
  server.use(
    http.post(`${TAURI}/get_tool_versions`, () =>
      HttpResponse.json(
        [
          "claude",
          "codex",
          "gemini",
          "grok",
          "opencode",
          "openclaw",
          "hermes",
        ].map((name) => ({
          name,
          version: installed.includes(name) ? "1.0.0" : null,
          latest_version: null,
          error: null,
          installed_but_broken: false,
          env_type: "linux",
          wsl_distro: null,
        })),
      ),
    ),
  );

const renderWizard = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <FirstRunWizard />
    </QueryClientProvider>,
  );
};

describe("onboarding model", () => {
  it("auto-opens only while the first-run flag is unset", () => {
    expect(shouldAutoOpenFirstRun(undefined)).toBe(false);
    expect(shouldAutoOpenFirstRun({} as Settings)).toBe(true);
    expect(
      shouldAutoOpenFirstRun({ firstRunNoticeConfirmed: true } as Settings),
    ).toBe(false);
  });

  it("pre-selects detected CLIs and keeps unprobed apps as configured", () => {
    const detected = detectedToolNames([
      { name: "claude", version: "1.0" },
      { name: "codex", version: null },
      { name: "grok", version: null, installed_but_broken: true },
    ]);
    expect([...detected].sort()).toEqual(["claude", "grok"]);

    const selection = initialSelection(DEFAULT_VISIBLE_APPS, detected);
    expect(selection.claude).toBe(true);
    expect(selection.grokbuild).toBe(true);
    expect(selection.codex).toBe(false);
    expect(selection.gemini).toBe(false);
    // Claude Desktop cannot be probed: current visibility wins
    expect(selection["claude-desktop"]).toBe(true);
  });

  it("keeps the current visibility when nothing is detected", () => {
    expect(initialSelection(undefined, new Set())).toEqual(
      DEFAULT_VISIBLE_APPS,
    );
    expect(initialSelection(undefined, null)).toEqual(DEFAULT_VISIBLE_APPS);
  });
});

describe("FirstRunWizard", () => {
  it("does not open once the first-run flag is set, but can be re-opened", async () => {
    setSettings({ firstRunNoticeConfirmed: true });
    mockToolVersions([]);
    renderWizard();

    await waitFor(() =>
      expect(screen.queryByTestId("onboarding-welcome")).toBeNull(),
    );
    act(() => openFirstRunWizard());
    expect(await screen.findByTestId("onboarding-welcome")).toBeInTheDocument();
  });

  it("skipping marks first run as done without touching app visibility", async () => {
    mockToolVersions(["claude"]);
    const user = userEvent.setup();
    renderWizard();

    await screen.findByTestId("onboarding-welcome");
    await user.click(screen.getByRole("button", { name: "Skip setup" }));

    await waitFor(() =>
      expect(getSettings().firstRunNoticeConfirmed).toBe(true),
    );
    expect(getSettings().visibleApps).toBeUndefined();
    expect(screen.queryByTestId("onboarding-welcome")).toBeNull();
  });

  it("walks through tools → providers → finish and saves visible apps", async () => {
    mockToolVersions(["claude", "codex"]);
    const user = userEvent.setup();
    renderWizard();

    await screen.findByTestId("onboarding-welcome");
    await user.click(screen.getByRole("button", { name: "Start setup" }));

    await screen.findByTestId("onboarding-tools");
    const codex = screen.getByRole("checkbox", { name: /codex/i });
    await waitFor(() => expect(codex).toHaveAttribute("data-state", "checked"));
    expect(screen.getByRole("checkbox", { name: /gemini/i })).toHaveAttribute(
      "data-state",
      "unchecked",
    );
    // the user unticks Codex
    await user.click(codex);
    await user.click(screen.getByRole("button", { name: "Next" }));

    await screen.findByTestId("onboarding-providers");
    expect(
      screen.getByTestId("onboarding-provider-claude"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("onboarding-provider-codex")).toBeNull();

    // add-provider opens the regular dialog for that app and returns to the wizard
    const claudeRow = screen.getByTestId("onboarding-provider-claude");
    await user.click(
      claudeRow.querySelector("button:last-of-type") as HTMLButtonElement,
    );
    const addDialog = await screen.findByTestId("add-provider-dialog");
    expect(addDialog).toHaveAttribute("data-app", "claude");
    await user.click(screen.getByRole("button", { name: "submit-provider" }));
    expect(addProviderMock).toHaveBeenCalledWith({ name: "x" });
    await screen.findByTestId("onboarding-providers");

    await user.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByTestId("onboarding-done");
    await user.click(screen.getByRole("button", { name: "Finish" }));

    await waitFor(() =>
      expect(getSettings().firstRunNoticeConfirmed).toBe(true),
    );
    const visible = getSettings().visibleApps!;
    expect(visible.claude).toBe(true);
    expect(visible.codex).toBe(false);
    expect(visible.gemini).toBe(false);
  });

  it("imports the existing live config for an app without providers", async () => {
    mockToolVersions(["gemini"]);
    let imported: string | null = null;
    server.use(
      http.post(`${TAURI}/get_providers`, () => HttpResponse.json({})),
      http.post(`${TAURI}/import_default_config`, async ({ request }) => {
        imported = ((await request.json()) as { app: string }).app;
        return HttpResponse.json(true);
      }),
    );
    const user = userEvent.setup();
    renderWizard();

    await user.click(
      await screen.findByRole("button", { name: "Start setup" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("checkbox", { name: /gemini/i })).toHaveAttribute(
        "data-state",
        "checked",
      ),
    );
    await user.click(screen.getByRole("button", { name: "Next" }));

    const row = await screen.findByTestId("onboarding-provider-gemini");
    await user.click(
      await within(row).findByRole("button", { name: /Import current config/ }),
    );
    await waitFor(() => expect(imported).toBe("gemini"));
    expect(
      await within(row).findByText(/saved as a provider named/),
    ).toBeInTheDocument();
  });
});
