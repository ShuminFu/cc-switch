import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { OpenCodeFormFields } from "@/components/providers/forms/OpenCodeFormFields";
import { Form } from "@/components/ui/form";

type OpenCodeFormFieldsProps = ComponentProps<typeof OpenCodeFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

const renderOpenCodeForm = (
  overrides: Partial<OpenCodeFormFieldsProps> = {},
) => {
  const props: OpenCodeFormFieldsProps = {
    npm: "@ai-sdk/openai-compatible",
    onNpmChange: vi.fn(),
    apiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    baseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    headers: {},
    onHeadersChange: vi.fn(),
    models: {
      "kimi-k2": {
        name: "Kimi K2",
        limit: { context: 1048576, output: 131072 },
      },
    },
    onModelsChange: vi.fn(),
    extraOptions: {},
    onExtraOptionsChange: vi.fn(),
    ...overrides,
  };

  return {
    props,
    ...render(
      <FormShell>
        <OpenCodeFormFields {...props} />
      </FormShell>,
    ),
  };
};

const expandFirstModel = () => {
  fireEvent.click(
    screen.getByRole("button", { name: "opencode.toggleModelDetails" }),
  );
};

describe("OpenCodeFormFields", () => {
  it("surfaces existing provider headers", () => {
    renderOpenCodeForm({
      headers: {
        "HTTP-Referer": "https://cc-switch.app",
        "X-Title": "CC Switch",
      },
    });

    expect(screen.getByDisplayValue("HTTP-Referer")).toBeInTheDocument();
    expect(
      screen.getByDisplayValue("https://cc-switch.app"),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("X-Title")).toBeInTheDocument();
    expect(screen.getByDisplayValue("CC Switch")).toBeInTheDocument();
  });

  it("updates provider headers", () => {
    const onHeadersChange = vi.fn();
    renderOpenCodeForm({
      headers: { "X-Title": "CC Switch" },
      onHeadersChange,
    });

    fireEvent.change(screen.getByDisplayValue("CC Switch"), {
      target: { value: "OpenCode" },
    });

    expect(onHeadersChange).toHaveBeenCalledWith({
      "X-Title": "OpenCode",
    });
  });

  it("shows a blank header name for newly added headers", () => {
    const onHeadersChange = vi.fn();
    const { rerender, props } = renderOpenCodeForm({ onHeadersChange });

    fireEvent.click(screen.getByRole("button", { name: "opencode.addHeader" }));

    const nextHeaders = onHeadersChange.mock.calls[0][0];
    const headerKey = Object.keys(nextHeaders)[0];
    expect(headerKey).toMatch(/^draft-header:/);

    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} headers={nextHeaders} />
      </FormShell>,
    );

    expect(
      screen.getByPlaceholderText("opencode.headerNamePlaceholder"),
    ).toHaveValue("");
  });

  it("removes provider headers", () => {
    const onHeadersChange = vi.fn();
    renderOpenCodeForm({
      headers: { "X-Title": "CC Switch" },
      onHeadersChange,
    });

    fireEvent.click(
      screen.getByRole("button", { name: "opencode.removeHeader" }),
    );

    expect(onHeadersChange).toHaveBeenCalledWith({});
  });

  it("rejects case-insensitive duplicate header names and restores the input", () => {
    const onHeadersChange = vi.fn();
    renderOpenCodeForm({
      headers: { "X-A": "A", "X-B": "B" },
      onHeadersChange,
    });

    const keyInput = screen.getByDisplayValue("X-B");
    fireEvent.change(keyInput, { target: { value: "x-a" } });
    fireEvent.blur(keyInput);

    expect(onHeadersChange).not.toHaveBeenCalled();
    expect(keyInput).toHaveValue("X-B");
  });

  it("surfaces provider options whose names start with option-", () => {
    renderOpenCodeForm({
      extraOptions: { "option-mode": "legacy" },
    });

    expect(screen.getByDisplayValue("option-mode")).toBeInTheDocument();
    expect(screen.getByDisplayValue("legacy")).toBeInTheDocument();
  });

  it("surfaces existing model token limits", () => {
    renderOpenCodeForm();

    expandFirstModel();

    expect(screen.getByLabelText("opencode.limitContext")).toHaveValue(1048576);
    expect(screen.getByLabelText("opencode.limitOutput")).toHaveValue(131072);
  });

  it("updates model token limits as structured numbers", () => {
    const onModelsChange = vi.fn();
    renderOpenCodeForm({ onModelsChange });

    expandFirstModel();
    fireEvent.change(screen.getByLabelText("opencode.limitContext"), {
      target: { value: "2000000" },
    });

    expect(onModelsChange).toHaveBeenCalledWith({
      "kimi-k2": {
        name: "Kimi K2",
        limit: { context: 2000000, output: 131072 },
      },
    });
  });

  it("removes model limit when both fields are cleared", () => {
    const onModelsChange = vi.fn();
    const { rerender, props } = renderOpenCodeForm({ onModelsChange });

    expandFirstModel();
    fireEvent.change(screen.getByLabelText("opencode.limitContext"), {
      target: { value: "" },
    });

    const withoutContext = {
      "kimi-k2": {
        name: "Kimi K2",
        limit: { output: 131072 },
      },
    };
    expect(onModelsChange).toHaveBeenLastCalledWith(withoutContext);

    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} models={withoutContext} />
      </FormShell>,
    );
    fireEvent.change(screen.getByLabelText("opencode.limitOutput"), {
      target: { value: "" },
    });

    expect(onModelsChange).toHaveBeenLastCalledWith({
      "kimi-k2": {
        name: "Kimi K2",
      },
    });
  });

  it("reports base URL and API key edits", () => {
    const onBaseUrlChange = vi.fn();
    const onApiKeyChange = vi.fn();
    renderOpenCodeForm({ onBaseUrlChange, onApiKeyChange });

    fireEvent.change(screen.getByDisplayValue("https://api.example.com/v1"), {
      target: { value: "https://relay.example.com/v1" },
    });
    fireEvent.change(screen.getByDisplayValue("sk-test"), {
      target: { value: "sk-new" },
    });

    expect(onBaseUrlChange).toHaveBeenCalledWith(
      "https://relay.example.com/v1",
    );
    expect(onApiKeyChange).toHaveBeenCalledWith("sk-new");
  });

  it("shows the empty state and appends a blank model", () => {
    const onModelsChange = vi.fn();
    renderOpenCodeForm({ models: {}, onModelsChange });

    expect(screen.getByText("opencode.noModels")).toBeInTheDocument();
    fireEvent.click(screen.getByText("opencode.addModel"));

    const [[nextModels]] = onModelsChange.mock.calls;
    expect(Object.values(nextModels)).toEqual([{ name: "" }]);
    expect(Object.keys(nextModels)[0]).toMatch(/^model-\d+$/);
  });

  it("renames and removes models", () => {
    const onModelsChange = vi.fn();
    renderOpenCodeForm({ onModelsChange });

    fireEvent.change(screen.getByDisplayValue("Kimi K2"), {
      target: { value: "Kimi K2.5" },
    });
    expect(onModelsChange).toHaveBeenLastCalledWith({
      "kimi-k2": {
        name: "Kimi K2.5",
        limit: { context: 1048576, output: 131072 },
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    expect(onModelsChange).toHaveBeenLastCalledWith({});
  });
});
