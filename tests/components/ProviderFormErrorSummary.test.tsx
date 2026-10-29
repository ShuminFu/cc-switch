import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import type { AppId } from "@/lib/api";
import { setSettings } from "../msw/state";

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
  appId: AppId,
  initialData?: React.ComponentProps<typeof ProviderForm>["initialData"],
) => {
  // Skip the one-time common-config notice; its modal hides the form.
  setSettings({ commonConfigConfirmed: true });
  const onSubmit = vi.fn();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const utils = render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId={appId}
        providerId={initialData ? "existing" : undefined}
        initialData={initialData}
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />
    </QueryClientProvider>,
  );
  return { ...utils, onSubmit };
};

const clickSave = () =>
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

const findSummary = () => screen.findByRole("alert");

describe("ProviderForm inline error summary (CCS-190)", () => {
  beforeEach(() => {
    toastErrorMock.mockReset();
  });

  it("lists a blocking Codex error inline and clears it once the next save passes", async () => {
    const { onSubmit } = renderForm("codex", {
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
    });

    const auth = screen.getAllByTestId("json-editor-json")[0];
    fireEvent.change(auth, { target: { value: "{bad" } });
    clickSave();

    const summary = await findSummary();
    expect(summary).toHaveAttribute("id", "provider-form-error-summary");
    expect(
      within(summary).getByText("providerForm.errorSummary.title"),
    ).toBeInTheDocument();
    expect(
      within(summary).getByText("providerForm.codexAuthInvalid"),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(auth, {
      target: { value: '{"OPENAI_API_KEY": "sk-fixed"}' },
    });
    clickSave();

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("blocks an OpenCode save without a provider key", async () => {
    const { onSubmit } = renderForm("opencode");

    clickSave();

    const summary = await findSummary();
    expect(
      within(summary).getByText("opencode.providerKeyRequired"),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("lists schema field errors, such as invalid settingsConfig JSON", async () => {
    const { onSubmit } = renderForm("openclaw", {
      name: "Claw",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://api.example.com/v1",
        apiKey: "sk-claw",
        api: "openai-completions",
        models: [],
      },
    });

    const editor = screen
      .getAllByTestId("json-editor-json")
      .find((el) => (el as HTMLTextAreaElement).value.includes("baseUrl"));
    expect(editor).toBeDefined();
    fireEvent.change(editor!, { target: { value: '{"baseUrl": ' } });
    clickSave();

    const summary = await findSummary();
    expect(summary.querySelector("ul")?.textContent).toMatch(/JSON/);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("blocks a Claude Desktop save without a name", async () => {
    const { onSubmit } = renderForm("claude-desktop");

    clickSave();

    const summary = await findSummary();
    expect(
      within(summary).getByText("providerForm.fillSupplierName"),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("blocks a Grok Build save with missing required fields", async () => {
    const { onSubmit } = renderForm("grokbuild");

    clickSave();

    const summary = await findSummary();
    expect(
      within(summary).getByText("providerForm.requiredFields"),
    ).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
