import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsLoadFailureNotice } from "@/components/app-shell/AppConfirmDialogs";
import { command } from "../msw/handlers";
import { server } from "../msw/server";

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));

vi.mock("sonner", () => ({ toast: toastMock }));

// 与真实 react-i18next 一致：t 引用在渲染间保持稳定
const i18nMock = vi.hoisted(() => ({
  t: (key: string, opts?: Record<string, unknown>) =>
    opts ? `${key} ${JSON.stringify(opts)}` : key,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => i18nMock,
}));

const renderNotice = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <SettingsLoadFailureNotice />
    </QueryClientProvider>,
  );
  return { invalidate };
};

describe("SettingsLoadFailureNotice", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders nothing and shows no toast when settings loaded fine", async () => {
    const checked = vi.fn(() => null);
    server.use(command("get_settings_load_failure", checked));

    renderNotice();

    await waitFor(() => expect(checked).toHaveBeenCalled());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(toastMock.warning).not.toHaveBeenCalled();
  });

  it("shows a persistent warning naming the file and backup path", async () => {
    server.use(
      command("get_settings_load_failure", () => ({
        path: "/home/u/.cc-switch/settings.json",
        error: "expected value at line 1 column 1",
        backupPath: "/home/u/.cc-switch/settings.json.corrupt-20260101-000000",
      })),
    );

    renderNotice();

    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));
    const [title, options] = toastMock.warning.mock.calls[0];
    expect(title).toBe("settings.loadFailure.backedUp");
    expect(options.description).toContain(
      "/home/u/.cc-switch/settings.json.corrupt-20260101-000000",
    );
    expect(options.description).toContain("/home/u/.cc-switch/settings.json");
    expect(options.closeButton).toBe(true);
    expect(options.duration).toBe(Infinity);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("asks for confirmation when no backup exists and acknowledges on confirm", async () => {
    const acknowledged = vi.fn(() => true);
    server.use(
      command("get_settings_load_failure", () => ({
        path: "/home/u/.cc-switch/settings.json",
        error: "Permission denied (os error 13)",
        backupPath: null,
      })),
      command("acknowledge_settings_load_failure", acknowledged),
    );

    const { invalidate } = renderNotice();

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("settings.loadFailure.blocked");
    expect(dialog).toHaveTextContent("/home/u/.cc-switch/settings.json");
    expect(dialog).toHaveTextContent("Permission denied (os error 13)");
    expect(acknowledged).not.toHaveBeenCalled();
    expect(toastMock.warning).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("confirm-dialog-confirm"));

    await waitFor(() => expect(acknowledged).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["settings"] }),
    );
    expect(toastMock.success).toHaveBeenCalledWith(
      "settings.loadFailure.acknowledged",
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("keeps saving blocked when the dialog is dismissed", async () => {
    const acknowledged = vi.fn(() => true);
    server.use(
      command("get_settings_load_failure", () => ({
        path: "/home/u/.cc-switch/settings.json",
        error: "Permission denied (os error 13)",
        backupPath: null,
      })),
      command("acknowledge_settings_load_failure", acknowledged),
    );

    renderNotice();

    await screen.findByRole("dialog");
    fireEvent.click(screen.getByText("settings.loadFailure.keepBlocked"));

    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(acknowledged).not.toHaveBeenCalled();
  });
});
