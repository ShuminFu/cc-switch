import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";

const modelFetchApiMock = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

vi.mock("@/components/providers/forms/EndpointSpeedTest", () => ({
  default: ({ onClose }: { onClose: () => void }) => (
    <div data-testid="endpoint-speed-test">
      <button type="button" onClick={onClose}>
        close-speed-test
      </button>
    </div>
  ),
}));

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const renderCodexFields = (overrides: Partial<CodexFormFieldsProps> = {}) => {
  const props: CodexFormFieldsProps = {
    codexApiKey: "",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: true,
    codexBaseUrl: "",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "",
    onModelChange: vi.fn(),
    apiFormat: "openai_chat",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    codexChatReasoning: {},
    onCodexChatReasoningChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    catalogModels: [],
    onCatalogModelsChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    ...overrides,
  };

  const utils = render(
    <FormShell>
      <CodexFormFields {...props} />
    </FormShell>,
  );
  return { ...utils, props };
};

describe("CodexFormFields", () => {
  it("hides the default model and advanced section for official providers", () => {
    renderCodexFields({ category: "official" });

    expect(
      screen.queryByLabelText("codexConfig.defaultModelLabel"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("providerForm.advancedOptionsToggle"),
    ).not.toBeInTheDocument();
  });

  it("forwards default model edits to the parent", () => {
    const { props } = renderCodexFields();

    fireEvent.change(screen.getByLabelText("codexConfig.defaultModelLabel"), {
      target: { value: "gpt-5" },
    });

    expect(props.onModelChange).toHaveBeenCalledWith("gpt-5");
  });

  it("keeps the advanced section collapsed until it has a value or is toggled", () => {
    renderCodexFields();

    expect(
      screen.getByText("codexConfig.advancedSectionHint"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("codexConfig.modelMappingTitle"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByText("providerForm.advancedOptionsToggle"));

    expect(
      screen.queryByText("codexConfig.advancedSectionHint"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("codexConfig.modelMappingTitle"),
    ).toBeInTheDocument();
  });

  it("shows the Anthropic-only controls and strips non-digits from max output tokens", () => {
    const { props } = renderCodexFields({ apiFormat: "anthropic" });

    expect(
      screen.getByLabelText("codexConfig.anthropicAuthFieldLabel"),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("switch", {
        name: "codexConfig.impersonateClaudeCodeLabel",
      }),
    );
    expect(props.onImpersonateClaudeCodeChange).toHaveBeenCalledWith(true);

    fireEvent.change(
      screen.getByLabelText("codexConfig.maxOutputTokensLabel"),
      { target: { value: "8192" } },
    );
    expect(props.onMaxOutputTokensChange).toHaveBeenCalledWith("8192");

    // Chat-only reasoning switches are not offered on the Anthropic path.
    expect(
      screen.queryByRole("switch", { name: "codexConfig.reasoningModeToggle" }),
    ).not.toBeInTheDocument();
  });

  it("turning on reasoning effort also enables thinking with the default effort param", () => {
    const { props } = renderCodexFields({
      codexChatReasoning: { supportsThinking: false },
    });
    fireEvent.click(screen.getByText("providerForm.advancedOptionsToggle"));

    fireEvent.click(
      screen.getByRole("switch", {
        name: "codexConfig.reasoningEffortToggle",
      }),
    );

    expect(props.onCodexChatReasoningChange).toHaveBeenCalledWith({
      supportsThinking: true,
      supportsEffort: true,
      effortParam: "reasoning_effort",
    });
  });

  it("reports catalog rows to the parent without the internal row id", async () => {
    const { props } = renderCodexFields();
    fireEvent.click(screen.getByText("providerForm.advancedOptionsToggle"));

    fireEvent.click(screen.getByText("codexConfig.addCatalogModel"));
    fireEvent.change(screen.getByLabelText("codexConfig.catalogColumnModel"), {
      target: { value: "deepseek-chat" },
    });

    await waitFor(() => {
      expect(props.onCatalogModelsChange).toHaveBeenLastCalledWith([
        { model: "deepseek-chat", displayName: "", contextWindow: "" },
      ]);
    });
  });

  it("offers to add a default model that is missing from the model mapping", async () => {
    const { props } = renderCodexFields({
      codexModel: "kimi-k2",
      catalogModels: [{ model: "deepseek-chat", displayName: "DeepSeek" }],
    });

    fireEvent.click(screen.getByText("codexConfig.addToModelMapping"));

    await waitFor(() => {
      expect(props.onCatalogModelsChange).toHaveBeenLastCalledWith([
        { model: "deepseek-chat", displayName: "DeepSeek", contextWindow: "" },
        { model: "kimi-k2", displayName: "kimi-k2", contextWindow: "" },
      ]);
    });
  });

  it("does not hit the network when fetching models without a base URL", () => {
    renderCodexFields({ codexApiKey: "sk-test" });

    fireEvent.click(screen.getByTitle("providerForm.fetchModels"));

    expect(modelFetchApiMock.fetchModelsForConfig).not.toHaveBeenCalled();
    expect(modelFetchApiMock.showFetchModelsError).toHaveBeenCalledWith(
      null,
      expect.any(Function),
      { hasApiKey: true, hasBaseUrl: false },
    );
  });

  it("fetches models with the current base URL, key and user agent", async () => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([
      { id: "gpt-5", ownedBy: "openai" },
    ]);
    renderCodexFields({
      codexApiKey: "sk-test",
      codexBaseUrl: "https://api.example.com/v1",
    });

    fireEvent.click(screen.getByTitle("providerForm.fetchModels"));

    await waitFor(() => {
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.example.com/v1",
        "sk-test",
        false,
        undefined,
        "",
      );
    });
  });

  it("renders the endpoint speed test only while the modal is open", () => {
    const { props, rerender } = renderCodexFields();
    expect(screen.queryByTestId("endpoint-speed-test")).not.toBeInTheDocument();

    rerender(
      <FormShell>
        <CodexFormFields {...props} isEndpointModalOpen />
      </FormShell>,
    );
    fireEvent.click(screen.getByText("close-speed-test"));

    expect(props.onEndpointModalToggle).toHaveBeenCalledWith(false);
  });
});
