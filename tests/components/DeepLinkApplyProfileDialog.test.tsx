import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DeepLinkApplyProfileDialog } from "@/components/DeepLinkApplyProfileDialog";
import { emitTauriEvent } from "../msw/tauriMocks";

const applyMock = vi.hoisted(() => vi.fn());
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));
vi.mock("@/lib/query/profiles", () => ({
  useApplyProfileMutation: () => ({
    mutateAsync: applyMock,
    isPending: false,
  }),
}));

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
  profileId: "prof-1",
  profileName: "Work Project",
  scope: "codex",
  currentProfileId: "prof-2",
  currentProfileName: "Side Project",
  alreadyCurrent: false,
};

describe("DeepLinkApplyProfileDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    applyMock.mockResolvedValue([]);
  });

  it("confirms before applying through the shared mutation", async () => {
    render(<DeepLinkApplyProfileDialog />);
    expect(screen.queryByTestId("deeplink-apply-profile-dialog")).toBeNull();

    await act(async () => {
      emitTauriEvent("deeplink-apply-profile", payload);
    });
    expect(
      screen.getByTestId("deeplink-apply-profile-current"),
    ).toHaveTextContent("Side Project");

    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    await waitFor(() =>
      expect(applyMock).toHaveBeenCalledWith({ id: "prof-1", scope: "codex" }),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("deeplink-apply-profile-dialog")).toBeNull(),
    );
  });

  it("only notifies when the profile is already current", async () => {
    render(<DeepLinkApplyProfileDialog />);
    await act(async () => {
      emitTauriEvent("deeplink-apply-profile", {
        ...payload,
        alreadyCurrent: true,
      });
    });
    expect(screen.queryByTestId("deeplink-apply-profile-dialog")).toBeNull();
    expect(toastMock.info).toHaveBeenCalledWith(
      "Work Project 已经是 codex 的当前项目",
    );
    expect(applyMock).not.toHaveBeenCalled();
  });
});
