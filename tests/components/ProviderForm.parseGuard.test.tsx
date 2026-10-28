import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { parse as parseToml } from "smol-toml";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";

const toastErrorMock = vi.hoisted(() => vi.fn());

vi.mock("sonner", () => ({
  toast: {
    error: toastErrorMock,
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

const renderForm = (
  props: Pick<
    React.ComponentProps<typeof ProviderForm>,
    "appId" | "initialData"
  >,
) => {
  const onSubmit = vi.fn();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const utils = render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        {...props}
        providerId="existing"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />
    </QueryClientProvider>,
  );
  return { ...utils, onSubmit };
};

const INITIAL_CODEX_CONFIG = `model_provider = "custom"
model = "gpt-5"

[model_providers.custom]
name = "custom"
base_url = "https://a.example.com/v1"
wire_api = "responses"
`;

const EDITED_CODEX_CONFIG = INITIAL_CODEX_CONFIG.replace(
  "https://a.example.com/v1",
  "https://b.example.com/v1",
);

const codexInitialData = {
  name: "Relay",
  category: "custom" as const,
  settingsConfig: {
    auth: { OPENAI_API_KEY: "sk-initial" },
    config: INITIAL_CODEX_CONFIG,
  },
};

const getCodexEditors = () => {
  const auth = screen.getAllByTestId("json-editor-json")[0];
  const config = screen
    .getAllByTestId("json-editor-javascript")
    .find((el) => (el as HTMLTextAreaElement).value.includes("base_url"));
  expect(config).toBeDefined();
  return { auth, config: config! };
};

const clickSave = () =>
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

describe("ProviderForm config parse guard", () => {
  beforeEach(() => {
    toastErrorMock.mockReset();
  });

  it("blocks Codex save when auth.json is invalid instead of saving the initial config", async () => {
    const { onSubmit } = renderForm({
      appId: "codex",
      initialData: codexInitialData,
    });

    const { auth, config } = getCodexEditors();
    fireEvent.change(config, { target: { value: EDITED_CODEX_CONFIG } });
    fireEvent.change(auth, { target: { value: "{bad" } });
    clickSave();

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("treats an empty Codex auth.json as {} and keeps the edited config", async () => {
    const { onSubmit } = renderForm({
      appId: "codex",
      initialData: codexInitialData,
    });

    const { auth, config } = getCodexEditors();
    fireEvent.change(config, { target: { value: EDITED_CODEX_CONFIG } });
    fireEvent.change(auth, { target: { value: "" } });
    clickSave();

    // Clearing auth.json also clears the API key, which is a soft issue:
    // confirm "save anyway" so the submit goes through.
    fireEvent.click(await screen.findByRole("button", { name: "仍要保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const settings = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(settings.auth).toEqual({});
    const toml = parseToml(settings.config) as any;
    expect(toml.model_providers.custom.base_url).toBe(
      "https://b.example.com/v1",
    );
  });

  it("blocks Codex save when config.toml is invalid", async () => {
    const { onSubmit } = renderForm({
      appId: "codex",
      initialData: codexInitialData,
    });

    const { config } = getCodexEditors();
    fireEvent.change(config, { target: { value: "[model_providers" } });
    clickSave();

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("blocks Gemini save when config JSON is invalid", async () => {
    const { onSubmit } = renderForm({
      appId: "gemini",
      initialData: {
        name: "Gemini Relay",
        category: "custom",
        settingsConfig: {
          env: {
            GOOGLE_GEMINI_BASE_URL: "https://g.example.com",
            GEMINI_API_KEY: "key",
          },
          config: { timeout: 1 },
        },
      },
    });

    const configEditor = screen
      .getAllByTestId("json-editor-json")
      .find((el) => (el as HTMLTextAreaElement).value.includes("timeout"));
    expect(configEditor).toBeDefined();
    fireEvent.change(configEditor!, { target: { value: '{"timeout": ' } });
    clickSave();

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
