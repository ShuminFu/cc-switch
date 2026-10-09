import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DeepLinkSwitchDialog } from "@/components/DeepLinkSwitchDialog";
import { providersApi } from "@/lib/api/providers";
import { emitTauriEvent } from "../msw/tauriMocks";

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      let text = (options?.defaultValue as string | undefined) ?? key;
      for (const [name, value] of Object.entries(options ?? {})) {
        text = text.split(`{{${name}}}`).join(String(value));
      }
      return text;
    },
  }),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) => (open ? <div>{children}</div> : null),
  DialogContent: ({ children, ...props }: any) => (
    <div {...props}>{children}</div>
  ),
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
  DialogDescription: ({ children }: any) => <p>{children}</p>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
}));

const payload = {
  app: "claude",
  providerId: "relay-1",
  providerName: "My Relay",
  currentProviderId: "official",
  currentProviderName: "Anthropic",
  alreadyCurrent: false,
};

const renderDialog = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <DeepLinkSwitchDialog />
    </QueryClientProvider>,
  );
};

describe("DeepLinkSwitchDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("stays hidden until a switch link arrives, then switches on confirm", async () => {
    const switchSpy = vi
      .spyOn(providersApi, "switch")
      .mockResolvedValue({ warnings: ["restart codex"] });
    renderDialog();
    expect(screen.queryByTestId("deeplink-switch-dialog")).toBeNull();

    await act(async () => {
      emitTauriEvent("deeplink-switch", payload);
    });

    expect(screen.getByTestId("deeplink-switch-dialog")).toBeInTheDocument();
    expect(
      screen.getByText("是否将 claude 切换到 My Relay？"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("deeplink-switch-current")).toHaveTextContent(
      "Anthropic",
    );

    fireEvent.click(screen.getByRole("button", { name: "切换" }));
    await waitFor(() =>
      expect(switchSpy).toHaveBeenCalledWith("relay-1", "claude"),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("deeplink-switch-dialog")).toBeNull(),
    );
    expect(toastMock.success).toHaveBeenCalledWith("claude 已切换到 My Relay");
    expect(toastMock.warning).toHaveBeenCalledWith("restart codex");
  });

  it("cancels without switching", async () => {
    const switchSpy = vi.spyOn(providersApi, "switch").mockResolvedValue({
      warnings: [],
    });
    renderDialog();
    await act(async () => {
      emitTauriEvent("deeplink-switch", payload);
    });
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() =>
      expect(screen.queryByTestId("deeplink-switch-dialog")).toBeNull(),
    );
    expect(switchSpy).not.toHaveBeenCalled();
  });

  it("only notifies when the target is already current", async () => {
    renderDialog();
    await act(async () => {
      emitTauriEvent("deeplink-switch", { ...payload, alreadyCurrent: true });
    });
    expect(screen.queryByTestId("deeplink-switch-dialog")).toBeNull();
    expect(toastMock.info).toHaveBeenCalledWith(
      "My Relay 已经是 claude 的当前供应商",
    );
  });

  it("keeps the dialog open and reports a failed switch", async () => {
    vi.spyOn(providersApi, "switch").mockRejectedValue(new Error("boom"));
    renderDialog();
    await act(async () => {
      emitTauriEvent("deeplink-switch", payload);
    });
    fireEvent.click(screen.getByRole("button", { name: "切换" }));
    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith("切换失败", {
        description: "boom",
      }),
    );
    expect(screen.getByTestId("deeplink-switch-dialog")).toBeInTheDocument();
  });
});
