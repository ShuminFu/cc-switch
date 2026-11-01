import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { OpenCodeFormFields } from "@/components/providers/forms/OpenCodeFormFields";
import { Form } from "@/components/ui/form";
import {
  OPENCODE_MODEL_DRAFT_PREFIX,
  OPENCODE_MODEL_FIELD_DRAFT_PREFIX,
  stripOpencodeModelDrafts,
} from "@/components/providers/forms/helpers/opencodeFormUtils";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn(), warning: vi.fn() },
}));

type Props = ComponentProps<typeof OpenCodeFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const renderFields = (overrides: Partial<Props> = {}) => {
  const props: Props = {
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
    models: { a: { name: "A" }, b: { name: "B" } },
    onModelsChange: vi.fn(),
    extraOptions: {},
    onExtraOptionsChange: vi.fn(),
    ...overrides,
  };
  render(
    <FormShell>
      <OpenCodeFormFields {...props} />
    </FormShell>,
  );
  return props;
};

describe("OpenCode model drafts (FE-PROV-20 / FE-PROV-28)", () => {
  it("adds new model rows with a draft key that is stripped on save", () => {
    const props = renderFields();
    // Several sections have an "Add" button; the model one adds a model.
    for (const button of screen.getAllByRole("button", {
      name: /^(opencode\.addModel|Add|Add Model|添加模型|添加)$/i,
    })) {
      if (vi.mocked(props.onModelsChange).mock.calls.length > 0) break;
      fireEvent.click(button);
    }
    const next = vi.mocked(props.onModelsChange).mock.calls[0][0];
    const newKey = Object.keys(next).find((key) => key !== "a" && key !== "b")!;
    expect(newKey.startsWith(OPENCODE_MODEL_DRAFT_PREFIX)).toBe(true);
    expect(Object.keys(stripOpencodeModelDrafts(next))).toEqual(["a", "b"]);
  });

  it("refuses to rename a model onto an existing id", () => {
    const props = renderFields();
    const idInput = screen.getByDisplayValue("b");
    fireEvent.change(idInput, { target: { value: "a" } });
    fireEvent.blur(idInput);
    expect(props.onModelsChange).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalled();
    expect(screen.getByDisplayValue("b")).toBeInTheDocument();
  });

  it("strips unnamed option / extra-field rows but keeps a key named option-x", () => {
    const stripped = stripOpencodeModelDrafts({
      m: {
        name: "M",
        [`${OPENCODE_MODEL_FIELD_DRAFT_PREFIX}1`]: "",
        options: {
          [`${OPENCODE_MODEL_FIELD_DRAFT_PREFIX}2`]: "",
          "option-x": 1,
        },
      } as never,
    });
    expect(stripped).toEqual({ m: { name: "M", options: { "option-x": 1 } } });
  });

  it("shows a real option key that starts with option-", () => {
    renderFields({
      models: { m: { name: "M", options: { "option-x": "v" } } },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Toggle model details" }),
    );
    expect(screen.getByDisplayValue("option-x")).toBeInTheDocument();
  });

  it("keeps typed option values as text until blur", () => {
    const props = renderFields({
      models: { m: { name: "M", options: { provider: "x" } } },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Toggle model details" }),
    );
    const valueInput = screen.getByDisplayValue("x");
    fireEvent.change(valueInput, { target: { value: '"abc"' } });
    expect(props.onModelsChange).not.toHaveBeenCalled();
    expect(screen.getByDisplayValue('"abc"')).toBeInTheDocument();
    fireEvent.blur(valueInput);
    expect(vi.mocked(props.onModelsChange).mock.calls[0][0].m.options).toEqual({
      provider: "abc",
    });
  });
});
