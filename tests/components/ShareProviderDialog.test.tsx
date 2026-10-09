import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ShareProviderDialog } from "@/components/providers/ShareProviderDialog";
import type { Provider } from "@/types";

const copyTextMock = vi.fn();
vi.mock("@/lib/clipboard", () => ({
  copyText: (...args: unknown[]) => copyTextMock(...args),
}));

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

const provider: Provider = {
  id: "p1",
  name: "Relay",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.relay.example/v1",
      ANTHROPIC_AUTH_TOKEN: "sk-secret",
    },
  },
};

describe("ShareProviderDialog", () => {
  beforeEach(() => {
    copyTextMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
  });

  it("omits the key by default and includes it after opting in", async () => {
    copyTextMock.mockResolvedValue(undefined);
    render(
      <ShareProviderDialog
        provider={provider}
        appId="claude"
        open
        onOpenChange={() => {}}
      />,
    );

    const link = screen.getByLabelText("深链") as HTMLTextAreaElement;
    expect(link.value.startsWith("ccswitch://v1/import?")).toBe(true);
    expect(link.value).not.toContain("sk-secret");

    fireEvent.click(screen.getByRole("checkbox"));
    expect(
      (screen.getByLabelText("深链") as HTMLTextAreaElement).value,
    ).toContain("apiKey=sk-secret");

    fireEvent.click(screen.getByRole("button", { name: /复制链接/ }));
    await waitFor(() => expect(copyTextMock).toHaveBeenCalledTimes(1));
    expect(copyTextMock.mock.calls[0][0]).toContain("apiKey=sk-secret");
    expect(toastSuccessMock).toHaveBeenCalled();
  });

  it("reports clipboard failures", async () => {
    copyTextMock.mockRejectedValue(new Error("denied"));
    render(
      <ShareProviderDialog
        provider={provider}
        appId="claude"
        open
        onOpenChange={() => {}}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /复制链接/ }));
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(String(toastErrorMock.mock.calls[0][0])).toContain("denied");
  });
});
