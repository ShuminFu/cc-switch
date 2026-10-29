import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HermesFormFields } from "@/components/providers/forms/HermesFormFields";
import { Form } from "@/components/ui/form";

const fetchModelsForConfigMock = vi.hoisted(() => vi.fn());
const showFetchModelsErrorMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: fetchModelsForConfigMock,
  showFetchModelsError: showFetchModelsErrorMock,
}));

vi.mock("sonner", () => ({
  toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() },
}));

type HermesFormFieldsProps = ComponentProps<typeof HermesFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const renderHermesForm = (overrides: Partial<HermesFormFieldsProps> = {}) => {
  const props: HermesFormFieldsProps = {
    baseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    apiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    apiMode: "chat_completions",
    onApiModeChange: vi.fn(),
    models: [{ id: "glm-5", name: "GLM 5", context_length: 200000 }],
    onModelsChange: vi.fn(),
    rateLimitDelay: undefined,
    onRateLimitDelayChange: vi.fn(),
    ...overrides,
  };
  return {
    props,
    ...render(
      <FormShell>
        <HermesFormFields {...props} />
      </FormShell>,
    ),
  };
};

const baseUrlInput = () =>
  screen.getByPlaceholderText("https://api.example.com/v1");

describe("HermesFormFields", () => {
  beforeEach(() => {
    fetchModelsForConfigMock.mockReset();
    showFetchModelsErrorMock.mockReset();
  });

  it("reports base URL edits", () => {
    const onBaseUrlChange = vi.fn();
    renderHermesForm({ onBaseUrlChange });

    fireEvent.change(baseUrlInput(), {
      target: { value: "https://relay.example.com/v1" },
    });

    expect(onBaseUrlChange).toHaveBeenCalledWith(
      "https://relay.example.com/v1",
    );
  });

  it.each([
    ["", "hermes.form.baseUrlRequired"],
    ["not a url", "hermes.form.baseUrlInvalid"],
    ["ftp://files.example.com", "hermes.form.baseUrlScheme"],
  ])("flags base URL %j after blur", (baseUrl, message) => {
    renderHermesForm({ baseUrl });

    // 未离开输入框前只显示提示，不显示错误
    expect(screen.queryByText(message)).not.toBeInTheDocument();
    fireEvent.blur(baseUrlInput());

    expect(screen.getByText(message)).toBeInTheDocument();
    expect(baseUrlInput()).toHaveAttribute("aria-invalid", "true");
  });

  it("accepts base URLs with template tokens", () => {
    renderHermesForm({ baseUrl: "https://${HOST}/v1" });
    fireEvent.blur(baseUrlInput());

    expect(baseUrlInput()).toHaveAttribute("aria-invalid", "false");
    expect(screen.getByText("hermes.form.baseUrlHint")).toBeInTheDocument();
  });

  it("keeps the API key editable for official presets", () => {
    const onApiKeyChange = vi.fn();
    renderHermesForm({ category: "official", apiKey: "", onApiKeyChange });

    const input = screen.getByPlaceholderText("providerForm.apiKeyAutoFill");
    expect(input).toBeEnabled();
    fireEvent.change(input, { target: { value: "sk-new" } });
    expect(onApiKeyChange).toHaveBeenCalledWith("sk-new");
  });

  it("labels the first model primary and the rest fallback", () => {
    renderHermesForm({
      models: [
        { id: "a", name: "A" },
        { id: "b", name: "B" },
      ],
    });

    expect(screen.getAllByText("hermes.form.primaryModel")).toHaveLength(1);
    expect(screen.getAllByText("hermes.form.fallbackModel")).toHaveLength(1);
  });

  it("shows the empty state and appends a blank model", () => {
    const onModelsChange = vi.fn();
    renderHermesForm({ models: [], onModelsChange });

    expect(screen.getByText("hermes.form.noModels")).toBeInTheDocument();
    fireEvent.click(screen.getByText("hermes.form.addModel"));

    expect(onModelsChange).toHaveBeenCalledWith([
      { id: "", name: "", context_length: undefined },
    ]);
  });

  it("updates and removes models", () => {
    const onModelsChange = vi.fn();
    renderHermesForm({ onModelsChange });

    fireEvent.change(screen.getByDisplayValue("glm-5"), {
      target: { value: "glm-5.1" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { id: "glm-5.1", name: "GLM 5", context_length: 200000 },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(onModelsChange).toHaveBeenLastCalledWith([]);
  });

  it("parses the context length as a number", () => {
    const onModelsChange = vi.fn();
    renderHermesForm({ onModelsChange });

    fireEvent.click(screen.getByText("hermes.form.advancedOptions"));
    fireEvent.change(screen.getByDisplayValue("200000"), {
      target: { value: "128000" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { id: "glm-5", name: "GLM 5", context_length: 128000 },
    ]);

    fireEvent.change(screen.getByDisplayValue("200000"), {
      target: { value: "" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { id: "glm-5", name: "GLM 5", context_length: undefined },
    ]);
  });

  it("opens the provider advanced section when a rate limit delay is set", () => {
    renderHermesForm({ rateLimitDelay: 0.5 });

    expect(screen.getByDisplayValue("0.5")).toBeInTheDocument();
  });

  it("keeps the rate limit delay non-negative", () => {
    const onRateLimitDelayChange = vi.fn();
    renderHermesForm({ rateLimitDelay: 0.5, onRateLimitDelayChange });
    const input = screen.getByDisplayValue("0.5");

    fireEvent.change(input, { target: { value: "1.5" } });
    expect(onRateLimitDelayChange).toHaveBeenLastCalledWith(1.5);

    fireEvent.change(input, { target: { value: "-1" } });
    expect(onRateLimitDelayChange).toHaveBeenLastCalledWith(undefined);

    fireEvent.change(input, { target: { value: "" } });
    expect(onRateLimitDelayChange).toHaveBeenLastCalledWith(undefined);
  });

  it("fetches models with the current base URL and key", async () => {
    fetchModelsForConfigMock.mockResolvedValue([
      { id: "glm-5-air", ownedBy: "zhipu" },
    ]);
    renderHermesForm();

    fireEvent.click(screen.getByText("providerForm.fetchModels"));

    expect(fetchModelsForConfigMock).toHaveBeenCalledWith(
      "https://api.example.com/v1",
      "sk-test",
    );
    // 拿到模型后每行出现选择下拉
    expect(
      await screen.findByRole("button", { name: "common.selectModel" }),
    ).toBeInTheDocument();
  });

  it("does not fetch models without an API key", async () => {
    renderHermesForm({ apiKey: "" });

    fireEvent.click(screen.getByText("providerForm.fetchModels"));

    await waitFor(() =>
      expect(showFetchModelsErrorMock).toHaveBeenCalledWith(
        null,
        expect.any(Function),
        { hasApiKey: false, hasBaseUrl: true },
      ),
    );
    expect(fetchModelsForConfigMock).not.toHaveBeenCalled();
  });
});
