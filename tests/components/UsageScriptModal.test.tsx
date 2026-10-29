import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  testScript: vi.fn(),
  saveSettings: vi.fn(),
  settings: { usageConfirmed: false } as Record<string, unknown>,
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: mocks.toastError,
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    usageApi: { ...actual.usageApi, testScript: mocks.testScript },
    settingsApi: { ...actual.settingsApi, save: mocks.saveSettings },
  };
});

vi.mock("@/lib/query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/query")>();
  return {
    ...actual,
    useSettingsQuery: () => ({ data: mocks.settings }),
  };
});

vi.mock("@/hooks/useDarkMode", () => ({
  useDarkMode: () => false,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id?: string;
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      data-testid={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const thirdPartyProvider: Provider = {
  id: "relay-1",
  name: "Relay",
  category: "third_party",
  settingsConfig: {
    env: {
      ANTHROPIC_AUTH_TOKEN: "sk-relay",
      ANTHROPIC_BASE_URL: "https://relay.example.com/",
    },
  },
};

type ModalProps = ComponentProps<typeof UsageScriptModal>;

const renderModal = (overrides: Partial<ModalProps> = {}) => {
  const props: ModalProps = {
    provider: thirdPartyProvider,
    appId: "claude",
    isOpen: true,
    onClose: vi.fn(),
    onSave: vi.fn(),
    ...overrides,
  };
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <UsageScriptModal {...props} />
    </QueryClientProvider>,
  );
  return props;
};

const enabledProvider = (
  usageScript: Record<string, unknown> = {},
): Provider => ({
  ...thirdPartyProvider,
  meta: {
    usage_script: {
      enabled: true,
      language: "javascript",
      code: "({ request: {}, extractor: function (r) { return r; } })",
      timeout: 10,
      templateType: "general",
      ...usageScript,
    },
  } as Provider["meta"],
});

describe("UsageScriptModal", () => {
  beforeEach(() => {
    mocks.settings = { usageConfirmed: false };
    mocks.saveSettings.mockResolvedValue(true);
  });

  describe("panel behaviour", () => {
    it("renders as a modal dialog named after the provider and traps focus", async () => {
      renderModal();

      const dialog = screen.getByRole("dialog", {
        name: "usageScript.title - Relay",
      });
      await waitFor(() => {
        expect(dialog).toContainElement(document.activeElement as HTMLElement);
      });
    });

    it("closes on Escape", () => {
      const props = renderModal();

      fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

      expect(props.onClose).toHaveBeenCalledTimes(1);
    });

    it("lets text inputs keep Escape instead of closing the panel", () => {
      const props = renderModal({ provider: enabledProvider() });

      fireEvent.keyDown(screen.getByLabelText("usageScript.timeoutSeconds"), {
        key: "Escape",
      });

      expect(props.onClose).not.toHaveBeenCalled();
    });

    it("closes via the back button", () => {
      const props = renderModal();

      fireEvent.click(screen.getByRole("button", { name: "common.back" }));

      expect(props.onClose).toHaveBeenCalledTimes(1);
    });

    it("renders nothing while closed", () => {
      renderModal({ isOpen: false });
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });

  describe("first-time enable confirmation", () => {
    it("asks for confirmation and Escape dismisses only the nested confirm dialog", async () => {
      const props = renderModal();

      fireEvent.click(
        screen.getByRole("switch", { name: "usageScript.enableUsageQuery" }),
      );
      const confirm = await screen.findByRole("dialog", {
        name: "confirm.usage.title",
      });

      fireEvent.keyDown(confirm, { key: "Escape" });

      await waitFor(() => {
        expect(screen.queryByText("confirm.usage.message")).toBeNull();
      });
      expect(props.onClose).not.toHaveBeenCalled();
      expect(
        screen.getByRole("switch", { name: "usageScript.enableUsageQuery" }),
      ).not.toBeChecked();
    });

    it("persists the confirmation and enables the script", async () => {
      renderModal();

      fireEvent.click(
        screen.getByRole("switch", { name: "usageScript.enableUsageQuery" }),
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "confirm.usage.confirm" }),
      );

      await waitFor(() => {
        expect(mocks.saveSettings).toHaveBeenCalledWith({
          usageConfirmed: true,
        });
      });
      expect(
        await screen.findByText("usageScript.presetTemplate"),
      ).toBeInTheDocument();
    });

    it("skips the confirmation once usage was confirmed before", () => {
      mocks.settings = { usageConfirmed: true };
      renderModal();

      fireEvent.click(
        screen.getByRole("switch", { name: "usageScript.enableUsageQuery" }),
      );

      expect(screen.queryByText("confirm.usage.message")).toBeNull();
      expect(
        screen.getByText("usageScript.presetTemplate"),
      ).toBeInTheDocument();
    });
  });

  describe("saving", () => {
    it("rejects an enabled script without a return statement", () => {
      const props = renderModal({ provider: enabledProvider() });

      fireEvent.change(screen.getByTestId("usage-code"), {
        target: { value: "({ request: {} })" },
      });
      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.saveConfig" }),
      );

      expect(mocks.toastError).toHaveBeenCalledWith(
        "usageScript.mustHaveReturn",
        { duration: 5000 },
      );
      expect(props.onSave).not.toHaveBeenCalled();
      expect(props.onClose).not.toHaveBeenCalled();
    });

    it("saves the script with the selected template and closes", () => {
      const props = renderModal({ provider: enabledProvider() });

      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.templateNewAPI" }),
      );
      fireEvent.change(screen.getByLabelText(/usageScript.accessToken/), {
        target: { value: "tok-1" },
      });
      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.saveConfig" }),
      );

      expect(props.onSave).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: true,
          templateType: "newapi",
          accessToken: "tok-1",
          apiKey: undefined,
        }),
      );
      expect(props.onClose).toHaveBeenCalledTimes(1);
    });

    it("does not require script code for native templates", () => {
      const props = renderModal({ provider: enabledProvider() });

      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.templateBalance" }),
      );
      expect(screen.queryByTestId("usage-code")).toBeNull();
      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.saveConfig" }),
      );

      expect(props.onSave).toHaveBeenCalledWith(
        expect.objectContaining({ templateType: "balance", code: "" }),
      );
    });
  });

  describe("testing the script", () => {
    it("runs the script against the backend and reports the summary", async () => {
      mocks.testScript.mockResolvedValue({
        success: true,
        data: [{ planName: "Pro", remaining: 12.5, unit: "USD" }],
      });
      renderModal({ provider: enabledProvider({ timeout: 7 }) });

      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.testScript" }),
      );

      await waitFor(() => {
        expect(mocks.toastSuccess).toHaveBeenCalled();
      });
      expect(mocks.testScript).toHaveBeenCalledWith(
        "relay-1",
        "claude",
        expect.stringContaining("extractor"),
        7,
        undefined,
        undefined,
        undefined,
        undefined,
        "general",
      );
      expect(mocks.toastSuccess.mock.calls[0][0]).toContain(
        "usageScript.testSuccess",
      );
    });

    it("surfaces a bare string rejection from the backend", async () => {
      mocks.testScript.mockRejectedValue("network down");
      renderModal({ provider: enabledProvider() });

      fireEvent.click(
        screen.getByRole("button", { name: "usageScript.testScript" }),
      );

      await waitFor(() => {
        expect(mocks.toastError).toHaveBeenCalledWith(
          "usageScript.testFailed: network down",
          { duration: 5000 },
        );
      });
    });
  });

  it("offers only the official subscription template for official providers", () => {
    mocks.settings = { usageConfirmed: true };
    renderModal({
      provider: {
        id: "claude-official",
        name: "Claude Official",
        category: "official",
        settingsConfig: { env: {} },
      },
    });
    fireEvent.click(
      screen.getByRole("switch", { name: "usageScript.enableUsageQuery" }),
    );

    const dialog = screen.getByRole("dialog");
    expect(
      within(dialog).getByRole("button", {
        name: "usageScript.templateOfficialSubscription",
      }),
    ).toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", {
        name: "usageScript.templateGeneral",
      }),
    ).toBeNull();
  });
});
