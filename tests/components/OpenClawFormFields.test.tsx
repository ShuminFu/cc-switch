import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { OpenClawFormFields } from "@/components/providers/forms/OpenClawFormFields";
import { Form } from "@/components/ui/form";
import type { OpenClawModel } from "@/types";

const fetchModelsForConfigMock = vi.hoisted(() => vi.fn());
const showFetchModelsErrorMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: fetchModelsForConfigMock,
  showFetchModelsError: showFetchModelsErrorMock,
}));

vi.mock("sonner", () => ({
  toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() },
}));

type OpenClawFormFieldsProps = ComponentProps<typeof OpenClawFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const MODEL: OpenClawModel = {
  id: "kimi-k2",
  name: "Kimi K2",
  contextWindow: 200000,
  maxTokens: 32000,
  cost: { input: 3, output: 15 },
  input: ["text"],
};

const renderOpenClawForm = (
  overrides: Partial<OpenClawFormFieldsProps> = {},
) => {
  const props: OpenClawFormFieldsProps = {
    baseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    apiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    api: "openai-completions",
    onApiChange: vi.fn(),
    models: [MODEL],
    onModelsChange: vi.fn(),
    userAgent: false,
    onUserAgentChange: vi.fn(),
    ...overrides,
  };
  return {
    props,
    ...render(
      <FormShell>
        <OpenClawFormFields {...props} />
      </FormShell>,
    ),
  };
};

const expandFirstModel = () =>
  fireEvent.click(screen.getByText("openclaw.advancedOptions"));

describe("OpenClawFormFields", () => {
  beforeEach(() => {
    fetchModelsForConfigMock.mockReset();
    showFetchModelsErrorMock.mockReset();
  });

  it("reports base URL edits", () => {
    const onBaseUrlChange = vi.fn();
    renderOpenClawForm({ onBaseUrlChange });

    fireEvent.change(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      { target: { value: "https://relay.example.com/v1" } },
    );

    expect(onBaseUrlChange).toHaveBeenCalledWith(
      "https://relay.example.com/v1",
    );
  });

  it("keeps the API key editable for official presets", () => {
    renderOpenClawForm({ category: "official", apiKey: "" });

    expect(
      screen.getByPlaceholderText("providerForm.apiKeyAutoFill"),
    ).toBeEnabled();
  });

  it("toggles the User-Agent switch", () => {
    const onUserAgentChange = vi.fn();
    renderOpenClawForm({ onUserAgentChange });

    // 第一个开关是 User-Agent（模型的推理开关在折叠区内）
    fireEvent.click(screen.getAllByRole("switch")[0]);

    expect(onUserAgentChange).toHaveBeenCalledWith(true);
  });

  it("shows the empty state and appends a text-input model", () => {
    const onModelsChange = vi.fn();
    renderOpenClawForm({ models: [], onModelsChange });

    expect(screen.getByText("openclaw.noModels")).toBeInTheDocument();
    fireEvent.click(screen.getByText("openclaw.addModel"));

    expect(onModelsChange).toHaveBeenCalledWith([
      {
        id: "",
        name: "",
        contextWindow: undefined,
        maxTokens: undefined,
        cost: undefined,
        input: ["text"],
      },
    ]);
  });

  it("labels the first model primary and the rest fallback", () => {
    renderOpenClawForm({ models: [MODEL, { ...MODEL, id: "kimi-k2-mini" }] });

    expect(screen.getAllByText("openclaw.primaryModel")).toHaveLength(1);
    expect(screen.getAllByText("openclaw.fallbackModel")).toHaveLength(1);
  });

  it("updates and removes models", () => {
    const onModelsChange = vi.fn();
    renderOpenClawForm({ onModelsChange });

    fireEvent.change(screen.getByDisplayValue("Kimi K2"), {
      target: { value: "Kimi K2.5" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { ...MODEL, name: "Kimi K2.5" },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(onModelsChange).toHaveBeenLastCalledWith([]);
  });

  it("parses token limits as integers and clears them when empty", () => {
    const onModelsChange = vi.fn();
    renderOpenClawForm({ onModelsChange });
    expandFirstModel();

    fireEvent.change(screen.getByDisplayValue("200000"), {
      target: { value: "128000" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { ...MODEL, contextWindow: 128000 },
    ]);

    fireEvent.change(screen.getByDisplayValue("32000"), {
      target: { value: "" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { ...MODEL, maxTokens: undefined },
    ]);
  });

  it("merges cost edits into the existing cost", () => {
    const onModelsChange = vi.fn();
    renderOpenClawForm({ onModelsChange });
    expandFirstModel();

    fireEvent.change(screen.getByPlaceholderText("0.3"), {
      target: { value: "0.25" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { ...MODEL, cost: { input: 3, output: 15, cacheRead: 0.25 } },
    ]);

    fireEvent.change(screen.getByDisplayValue("15"), {
      target: { value: "" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { ...MODEL, cost: { input: 3, output: undefined } },
    ]);
  });

  it("toggles reasoning and input types", () => {
    const onModelsChange = vi.fn();
    renderOpenClawForm({ onModelsChange });
    expandFirstModel();

    expect(screen.getByText("openclaw.reasoningOff")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("switch")[1]);
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { ...MODEL, reasoning: true },
    ]);

    const [textBox, imageBox] = screen.getAllByRole("checkbox");
    expect(textBox).toBeChecked();
    expect(imageBox).not.toBeChecked();

    fireEvent.click(imageBox);
    expect(onModelsChange).toHaveBeenLastCalledWith([
      { ...MODEL, input: ["text", "image"] },
    ]);

    // text 也可以取消：有的模型不支持文本输入
    fireEvent.click(textBox);
    expect(onModelsChange).toHaveBeenLastCalledWith([{ ...MODEL, input: [] }]);
  });

  it("fetches models with the current base URL and key", async () => {
    fetchModelsForConfigMock.mockResolvedValue([
      { id: "kimi-k2-turbo", ownedBy: "moonshot" },
    ]);
    renderOpenClawForm();

    fireEvent.click(screen.getByText("providerForm.fetchModels"));

    expect(fetchModelsForConfigMock).toHaveBeenCalledWith(
      "https://api.example.com/v1",
      "sk-test",
    );
    expect(
      await screen.findByRole("button", { name: "common.selectModel" }),
    ).toBeInTheDocument();
  });

  it("does not fetch models without a base URL", async () => {
    renderOpenClawForm({ baseUrl: "" });

    fireEvent.click(screen.getByText("providerForm.fetchModels"));

    await waitFor(() =>
      expect(showFetchModelsErrorMock).toHaveBeenCalledWith(
        null,
        expect.any(Function),
        { hasApiKey: true, hasBaseUrl: false },
      ),
    );
    expect(fetchModelsForConfigMock).not.toHaveBeenCalled();
  });
});
