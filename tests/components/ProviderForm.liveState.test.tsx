import type { ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
    id,
  }: {
    value: string;
    onChange: (value: string) => void;
    id?: string;
  }) => (
    <textarea
      data-testid={id ? `json-editor-${id}` : "json-editor"}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

// Rendering the full ProviderForm is slow on a loaded CI machine.
vi.setConfig({ testTimeout: 30000 });

function renderWithClient(node: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>{node}</QueryClientProvider>,
  );
}

describe("ProviderForm live settingsConfig (FE-PROV-01)", () => {
  it("keeps JSON-editor edits when the API key is typed afterwards", async () => {
    const onSubmit = vi.fn<(values: ProviderFormValues) => void>();

    renderWithClient(
      <ProviderForm
        appId="claude"
        providerId="custom-1"
        submitLabel="save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Custom",
          category: "custom",
          settingsConfig: {
            env: {
              ANTHROPIC_BASE_URL: "https://api.example.com",
              ANTHROPIC_AUTH_TOKEN: "old-key",
            },
          },
        }}
      />,
    );

    const editor = (await screen.findAllByTestId(/json-editor/)).find(
      (element) =>
        (element as HTMLTextAreaElement).value.includes("ANTHROPIC_BASE_URL"),
    ) as HTMLTextAreaElement;
    expect(editor).toBeTruthy();
    const apiKeyInput = (await screen.findByDisplayValue(
      "old-key",
    )) as HTMLInputElement;
    // Let pending queries settle so no unrelated re-render refreshes the
    // form between the JSON edit and the API-key keystroke.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 100));
    });

    const edited = JSON.stringify(
      {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "old-key",
          MY_CUSTOM_FLAG: "1",
        },
      },
      null,
      2,
    );
    fireEvent.change(editor, { target: { value: edited } });
    fireEvent.change(apiKeyInput, { target: { value: "new-key" } });

    await waitFor(() => {
      const parsed = JSON.parse(editor.value);
      expect(parsed.env.ANTHROPIC_AUTH_TOKEN).toBe("new-key");
      expect(parsed.env.MY_CUSTOM_FLAG).toBe("1");
    });

    fireEvent.submit(document.getElementById("provider-form")!);

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(submitted.env).toMatchObject({
      ANTHROPIC_AUTH_TOKEN: "new-key",
      MY_CUSTOM_FLAG: "1",
    });
  });
});

describe("ProviderForm editor errors block save (FE-PROV-08)", () => {
  it("does not submit when Codex auth.json is invalid", async () => {
    const onSubmit = vi.fn();

    renderWithClient(
      <ProviderForm
        appId="codex"
        providerId="codex-1"
        submitLabel="save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Codex Custom",
          category: "custom",
          settingsConfig: {
            auth: { OPENAI_API_KEY: "sk-test" },
            config: 'model_provider = "custom"\nmodel = "gpt-5"\n',
          },
        }}
      />,
    );

    const authEditor = (await screen.findAllByTestId(/json-editor/)).find(
      (element) =>
        (element as HTMLTextAreaElement).value.includes("OPENAI_API_KEY"),
    ) as HTMLTextAreaElement;
    expect(authEditor).toBeTruthy();

    fireEvent.change(authEditor, {
      target: { value: '{ "OPENAI_API_KEY": "sk-test", ' },
    });

    fireEvent.submit(document.getElementById("provider-form")!);

    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("does not submit when Gemini config.json is invalid", async () => {
    const onSubmit = vi.fn();

    renderWithClient(
      <ProviderForm
        appId="gemini"
        providerId="gemini-1"
        submitLabel="save"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Gemini Custom",
          category: "custom",
          settingsConfig: {
            env: { GEMINI_API_KEY: "g-key" },
            config: { general: { previewFeatures: true } },
          },
        }}
      />,
    );

    const configEditor = (await screen.findAllByTestId(/json-editor/)).find(
      (element) =>
        (element as HTMLTextAreaElement).value.includes("previewFeatures"),
    ) as HTMLTextAreaElement;
    expect(configEditor).toBeTruthy();

    fireEvent.change(configEditor, { target: { value: "{ not json" } });

    fireEvent.submit(document.getElementById("provider-form")!);

    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

describe("ProviderForm partner presets (FE-PROV-43)", () => {
  it("persists isPartner and partnerPromotionKey without opening the endpoint panel", async () => {
    const onSubmit = vi.fn<(values: ProviderFormValues) => void>();

    renderWithClient(
      <ProviderForm
        appId="claude"
        submitLabel="add"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: /PatewayAI/ }));

    const apiKeyInput = await screen.findByLabelText(/API Key/);
    fireEvent.change(apiKeyInput, { target: { value: "sk-partner" } });

    fireEvent.submit(document.getElementById("provider-form")!);

    const saveAnyway = await screen
      .findByRole("button", { name: "Save anyway" }, { timeout: 500 })
      .catch(() => null);
    if (saveAnyway) fireEvent.click(saveAnyway);

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.meta?.isPartner).toBe(true);
    expect(submitted.meta?.partnerPromotionKey).toBe("patewayai");
    expect(submitted.meta?.custom_endpoints).toBeUndefined();
  });
});
