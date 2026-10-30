import {
  render,
  screen,
  within,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CAPABILITY_COLUMNS,
  CapabilityMatrix,
  formatCapabilityMatrix,
} from "@/components/settings/CapabilityMatrix";
import { ALL_APP_IDS } from "@/entities/app/registry";
import type { BackendAppDescriptor } from "@/lib/api/apps";
import registryFixture from "../fixtures/app-registry.json";
import { command } from "../msw/handlers";
import { server } from "../msw/server";

// 与 src-tauri/src/apps/registry.rs 的 registry_dto_matches_shared_fixture 共用同一份快照
const fixture = registryFixture as BackendAppDescriptor[];

const copyTextMock = vi.fn<(text: string) => Promise<void>>();
vi.mock("@/lib/clipboard", () => ({
  copyText: (text: string) => copyTextMock(text),
}));

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const SUPPORTED = "settings.advanced.capabilityMatrix.supported";
const UNSUPPORTED = "settings.advanced.capabilityMatrix.unsupported";

const renderMatrix = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <CapabilityMatrix />
    </QueryClientProvider>,
  );
};

describe("CapabilityMatrix", () => {
  beforeEach(() => {
    copyTextMock.mockReset();
    copyTextMock.mockResolvedValue(undefined);
  });

  it("fixture covers every app in registry order", () => {
    expect(fixture.map((app) => app.id)).toEqual([...ALL_APP_IDS]);
    expect(fixture).toHaveLength(8);
  });

  it("renders one row per app with cells matching the registry fixture", async () => {
    renderMatrix();

    const table = await screen.findByRole("table");
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((th) => th.textContent);
    expect(headers).toEqual([
      "settings.advanced.capabilityMatrix.app",
      ...CAPABILITY_COLUMNS.map(
        (cap) => `settings.advanced.capabilityMatrix.caps.${cap}`,
      ),
    ]);

    const bodyRows = within(table).getAllByRole("row").slice(1);
    expect(bodyRows).toHaveLength(fixture.length);

    fixture.forEach((app, index) => {
      const row = bodyRows[index];
      expect(within(row).getByRole("rowheader")).toHaveTextContent(
        app.displayName,
      );
      const cells = within(row).getAllByRole("cell");
      expect(cells.map((cell) => cell.textContent)).toEqual(
        CAPABILITY_COLUMNS.map((cap) =>
          app.caps[cap] ? SUPPORTED : UNSUPPORTED,
        ),
      );
    });
  });

  it("reflects caps exposed only by the backend (usage, session resume)", async () => {
    renderMatrix();
    const desktopRow = (await screen.findByText("Claude Desktop")).closest(
      "tr",
    )!;
    const cell = (cap: string) =>
      desktopRow.querySelector(`[data-cap="${cap}"]`)!;
    expect(cell("usage")).toHaveTextContent(SUPPORTED);
    expect(cell("sessionResume")).toHaveTextContent(UNSUPPORTED);
    expect(cell("mcp")).toHaveTextContent(UNSUPPORTED);
  });

  it("copies a plain-text diagnostics table", async () => {
    renderMatrix();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.advanced.capabilityMatrix.copy",
      }),
    );

    await waitFor(() => expect(copyTextMock).toHaveBeenCalledTimes(1));
    const text = copyTextMock.mock.calls[0][0];
    expect(text).toBe(formatCapabilityMatrix(fixture));
    expect(text).toContain(
      "| app | mcp | skills | prompts | proxyTakeover | usage | sessionResume |",
    );
    expect(text).toContain("| claude | yes | yes | yes | yes | yes | yes |");
    expect(text).toContain("| openclaw | yes | no | yes | yes | yes | no |");
    for (const id of ALL_APP_IDS) {
      expect(text).toContain(`| ${id} |`);
    }
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.advanced.capabilityMatrix.copied",
    );
  });

  it("reports a copy failure", async () => {
    copyTextMock.mockRejectedValue(new Error("denied"));
    renderMatrix();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.advanced.capabilityMatrix.copy",
      }),
    );
    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.advanced.capabilityMatrix.copyFailed",
        { description: "denied" },
      ),
    );
  });

  it("shows an error when the registry cannot be loaded", async () => {
    server.use(
      command("get_app_registry", () =>
        HttpResponse.json("boom", { status: 500 }),
      ),
    );
    renderMatrix();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "settings.advanced.capabilityMatrix.loadFailed",
    );
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });
});
