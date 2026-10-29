import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import type { Provider } from "@/types";
import { setSettings } from "../msw/state";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
    language,
  }: {
    value: string;
    onChange: (value: string) => void;
    language?: string;
  }) => (
    <textarea
      data-testid={`json-editor-${language ?? "json"}`}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const withQueryClient = (ui: React.ReactElement) => {
  // 跳过一次性的通用配置提示，其弹窗会遮住表单
  setSettings({ commonConfigConfirmed: true });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
};

const renderAddPanel = () => {
  const onOpenChange = vi.fn();
  withQueryClient(
    <AddProviderDialog
      open
      onOpenChange={onOpenChange}
      appId="claude"
      onSubmit={vi.fn()}
    />,
  );
  return { onOpenChange };
};

const renderCodexEditPanel = (onOpenChange: (open: boolean) => void) => {
  const provider: Provider = {
    id: "relay",
    name: "Relay",
    category: "custom",
    settingsConfig: {
      auth: { OPENAI_API_KEY: "sk-initial" },
      config: `model_provider = "custom"
model = "gpt-5"

[model_providers.custom]
name = "custom"
base_url = "https://a.example.com/v1"
wire_api = "responses"
`,
    },
  };

  withQueryClient(
    <EditProviderDialog
      open
      provider={provider}
      onOpenChange={onOpenChange}
      onSubmit={vi.fn()}
      appId="codex"
      isProxyTakeover
    />,
  );
};

// Radix 在 document 上监听 Esc；从 body 派发，模拟焦点不在输入框时按 Esc
const pressEscape = () => fireEvent.keyDown(document.body, { key: "Escape" });

const confirmTitle = "providerForm.unsavedChanges.title";

const getNameInput = async () =>
  (await screen.findByLabelText("provider.name")) as HTMLInputElement;

describe("provider panel unsaved changes guard", () => {
  it("asks before Esc discards edits, and keeps them on cancel", async () => {
    const { onOpenChange } = renderAddPanel();

    const nameInput = await getNameInput();
    fireEvent.change(nameInput, { target: { value: "My Relay" } });

    pressEscape();

    expect(await screen.findByText(confirmTitle)).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.unsavedChanges.keepEditing",
      }),
    );

    await waitFor(() =>
      expect(screen.queryByText(confirmTitle)).not.toBeInTheDocument(),
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(((await getNameInput()) as HTMLInputElement).value).toBe("My Relay");
  });

  it("closes after the user confirms discarding the edits", async () => {
    const { onOpenChange } = renderAddPanel();

    fireEvent.change(await getNameInput(), {
      target: { value: "My Relay" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.back" }));

    expect(await screen.findByText(confirmTitle)).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.unsavedChanges.discard",
      }),
    );

    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes without asking when nothing was edited", async () => {
    const { onOpenChange } = renderAddPanel();
    await getNameInput();

    pressEscape();

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(screen.queryByText(confirmTitle)).not.toBeInTheDocument();
  });

  it("closes the edit panel without asking when nothing was edited", async () => {
    const onOpenChange = vi.fn();
    renderCodexEditPanel(onOpenChange);
    await screen.findAllByTestId("json-editor-javascript");

    pressEscape();

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(screen.queryByText(confirmTitle)).not.toBeInTheDocument();
  });

  it("treats edits to a config editor outside react-hook-form as unsaved", async () => {
    const onOpenChange = vi.fn();
    renderCodexEditPanel(onOpenChange);

    const configEditor = (
      await screen.findAllByTestId("json-editor-javascript")
    ).find((el) => (el as HTMLTextAreaElement).value.includes("base_url"));
    expect(configEditor).toBeDefined();
    fireEvent.change(configEditor!, {
      target: {
        value: (configEditor as HTMLTextAreaElement).value.replace(
          "a.example.com",
          "b.example.com",
        ),
      },
    });

    pressEscape();

    expect(await screen.findByText(confirmTitle)).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});
