import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OpenClawFormFields } from "@/components/providers/forms/OpenClawFormFields";
import type { OpenClawModel } from "@/types";
import { useForm } from "react-hook-form";
import { Form } from "@/components/ui/form";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});

function Harness({ onModels }: { onModels: (m: OpenClawModel[]) => void }) {
  const form = useForm();
  const [models, setModels] = useState<OpenClawModel[]>([
    { id: "m1", name: "Model 1", contextWindow: 200000 },
  ]);
  return (
    <Form {...form}>
      <OpenClawFormFields
        baseUrl=""
        onBaseUrlChange={vi.fn()}
        apiKey=""
        onApiKeyChange={vi.fn()}
        shouldShowApiKeyLink={false}
        websiteUrl=""
        api="openai-completions"
        onApiChange={vi.fn()}
        models={models}
        onModelsChange={(next) => {
          setModels(next);
          onModels(next);
        }}
        userAgent={false}
        onUserAgentChange={vi.fn()}
      />
    </Form>
  );
}

describe("OpenClawFormFields integer inputs", () => {
  it("parses context window strictly and keeps the stored number format", () => {
    const onModels = vi.fn();
    render(<Harness onModels={onModels} />);
    fireEvent.click(screen.getByText("openclaw.advancedOptions"));

    const contextInput = screen.getByDisplayValue("200000");

    fireEvent.change(contextInput, { target: { value: "1e5" } });
    expect(onModels).toHaveBeenLastCalledWith([
      expect.objectContaining({ contextWindow: 100000 }),
    ]);

    onModels.mockClear();
    fireEvent.change(contextInput, { target: { value: "12abc" } });
    expect(onModels).not.toHaveBeenCalled();
    expect(screen.getByText("common.integerInvalid")).toBeInTheDocument();

    fireEvent.change(contextInput, { target: { value: "" } });
    expect(onModels).toHaveBeenLastCalledWith([
      expect.objectContaining({ contextWindow: undefined }),
    ]);
  });

  it("rejects zero / negative max tokens", () => {
    const onModels = vi.fn();
    render(<Harness onModels={onModels} />);
    fireEvent.click(screen.getByText("openclaw.advancedOptions"));

    fireEvent.change(screen.getByPlaceholderText("32000"), {
      target: { value: "-5" },
    });
    expect(onModels).not.toHaveBeenCalled();
    expect(screen.getByText("common.integerMin")).toBeInTheDocument();
  });
});
