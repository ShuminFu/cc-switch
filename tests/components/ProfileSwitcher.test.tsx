/**
 * ProfileSwitcher (TEST-18): MSW-backed, real en.json strings, assertions on
 * the exact IPC payloads.
 */
import type { ReactNode } from "react";
import i18n from "i18next";
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { AgentsPanel } from "@/components/agents/AgentsPanel";
import { mockInvoke } from "../msw/invokeRecorder";
import { createTestQueryClient } from "../utils/testQueryClient";

const emptySlots = {
  claude: null,
  "claude-desktop": null,
  codex: null,
  gemini: null,
  grokbuild: null,
  opencode: null,
  openclaw: null,
  hermes: null,
};

const profile = (id: string, name: string, withClaude = true) => ({
  id,
  name,
  payload: {
    providers: { ...emptySlots, claude: withClaude ? "p1" : null },
    mcp: emptySlots,
    skills: emptySlots,
    prompts: emptySlots,
  },
});

function listing(currentClaude: string | null) {
  return {
    profiles: [profile("work", "Work"), profile("home", "Home", false)],
    currentIds: {
      claude: currentClaude,
      claudeDesktop: null,
      codex: null,
      gemini: null,
      grokbuild: null,
      opencode: null,
      openclaw: null,
      hermes: null,
    },
  };
}

/** The trigger is a combobox (named by its title), showing the current project. */
async function findCombobox(label: string) {
  await waitFor(() =>
    expect(screen.getByRole("combobox").textContent).toContain(label),
  );
  return screen.getByRole("combobox");
}

function renderSwitcher(ui: ReactNode) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>{ui}</QueryClientProvider>,
  );
}

beforeEach(async () => {
  i18n.addResourceBundle("en", "translation", en, true, true);
  await i18n.changeLanguage("en");
});

afterEach(() => {
  i18n.removeResourceBundle("en", "translation");
  i18n.addResourceBundle("en", "translation", {});
});

describe("ProfileSwitcher", () => {
  it("shows the current project name, or 'No project'", async () => {
    mockInvoke({ list_profiles: listing("work") });
    renderSwitcher(<ProfileSwitcher activeApp="claude" />);
    expect(await findCombobox("Work")).toBeInTheDocument();
  });

  it("applies a different project for the active app's scope", async () => {
    const recorder = mockInvoke({
      list_profiles: listing(null),
      apply_profile: [],
    });
    renderSwitcher(<ProfileSwitcher activeApp="claude" />);
    const user = userEvent.setup();
    await user.click(await findCombobox("No project"));
    await user.click(await screen.findByRole("option", { name: /Work/ }));

    await waitFor(() =>
      expect(recorder.callsTo("apply_profile")).toEqual([
        { command: "apply_profile", args: { id: "work", scope: "claude" } },
      ]),
    );
  });

  it("does not re-apply the project that is already current", async () => {
    const recorder = mockInvoke({
      list_profiles: listing("work"),
      apply_profile: [],
    });
    renderSwitcher(<ProfileSwitcher activeApp="claude" />);
    const user = userEvent.setup();
    await user.click(await findCombobox("Work"));
    await user.click(await screen.findByRole("option", { name: /Work/ }));
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(recorder.callsTo("apply_profile")).toEqual([]);
  });

  it("marks projects without a snapshot for this scope", async () => {
    mockInvoke({ list_profiles: listing(null) });
    renderSwitcher(<ProfileSwitcher activeApp="claude" />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("combobox"));
    const home = await screen.findByRole("option", { name: /Home/ });
    expect(home.textContent).toContain(en.profiles.noSnapshotForScope);
    const work = screen.getByRole("option", { name: /Work/ });
    expect(work.textContent).not.toContain(en.profiles.noSnapshotForScope);
  });

  it("creates a project from the current state with a trimmed name", async () => {
    const recorder = mockInvoke({
      list_profiles: listing(null),
      create_profile: profile("new", "Client A"),
    });
    renderSwitcher(<ProfileSwitcher activeApp="codex" />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("combobox"));
    await user.click(
      await screen.findByRole("option", { name: "New project" }),
    );

    const confirm = await screen.findByRole("button", { name: "Confirm" });
    expect(confirm).toBeDisabled();
    await user.type(screen.getByRole("textbox"), "  Client A  ");
    await user.click(confirm);

    await waitFor(() =>
      expect(recorder.callsTo("create_profile")).toEqual([
        {
          command: "create_profile",
          args: { name: "Client A", scope: "codex" },
        },
      ]),
    );
  });

  it("clears the current project for the scope", async () => {
    const recorder = mockInvoke({
      list_profiles: listing("work"),
      clear_current_profile: [],
    });
    renderSwitcher(<ProfileSwitcher activeApp="claude" />);
    const user = userEvent.setup();
    await user.click(await findCombobox("Work"));
    await user.click(await screen.findByRole("option", { name: "No project" }));
    await waitFor(() =>
      expect(recorder.callsTo("clear_current_profile")).toEqual([
        { command: "clear_current_profile", args: { scope: "claude" } },
      ]),
    );
  });
});

describe("AgentsPanel", () => {
  it("renders the coming-soon placeholder", () => {
    render(<AgentsPanel onOpenChange={() => {}} />);
    expect(screen.getByText("Coming Soon")).toBeInTheDocument();
  });
});
