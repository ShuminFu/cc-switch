import { useState } from "react";
import { useForm } from "react-hook-form";
import { Form } from "@/components/ui/form";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OpenClawFormFields } from "@/components/providers/forms/OpenClawFormFields";
import { moveItem } from "@/components/providers/forms/shared/ModelOrderButtons";
import type { OpenClawModel } from "@/types";

const onModelsSpy = vi.fn();

function Harness({ initial }: { initial: OpenClawModel[] }) {
  const [models, setModels] = useState(initial);
  const form = useForm();
  return (
    <Form {...form}>
      <OpenClawFormFields
        baseUrl=""
        onBaseUrlChange={() => {}}
        apiKey=""
        onApiKeyChange={() => {}}
        shouldShowApiKeyLink={false}
        websiteUrl=""
        api="openai-completions"
        onApiChange={() => {}}
        models={models}
        onModelsChange={(next) => {
          onModelsSpy(next);
          setModels(next);
        }}
        userAgent={false}
        onUserAgentChange={() => {}}
      />
    </Form>
  );
}

const rowTriggers = () =>
  Array.from(
    document.querySelectorAll<HTMLButtonElement>(
      "button[aria-expanded][data-state]:not([role=combobox])",
    ),
  );

describe("OpenClaw model rows (FE-PROV-11)", () => {
  it("moveItem moves one element", () => {
    expect(moveItem(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    expect(moveItem(["a"], 0, 1)).toEqual(["a"]);
  });

  it("keeps the expanded row expanded after deleting the row above it", () => {
    render(
      <Harness
        initial={[
          { id: "m1", name: "M1" },
          { id: "m2", name: "M2" },
        ]}
      />,
    );
    expect(rowTriggers()).toHaveLength(2);
    fireEvent.click(rowTriggers()[1]);
    expect(rowTriggers()[1]).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(
      screen.getAllByRole("button", { name: /Remove model|removeModel/ })[0],
    );
    expect(screen.getByDisplayValue("m2")).toBeInTheDocument();
    expect(rowTriggers()).toHaveLength(1);
    expect(rowTriggers()[0]).toHaveAttribute("aria-expanded", "true");
  });

  it("a move-up action changes the default (first) model", () => {
    onModelsSpy.mockClear();
    render(
      <Harness
        initial={[
          { id: "m1", name: "M1" },
          { id: "m2", name: "M2" },
        ]}
      />,
    );
    const upButtons = screen.getAllByRole("button", {
      name: /Move model up|moveModelUp/,
    });
    expect(upButtons[0]).toBeDisabled();
    fireEvent.click(upButtons[1]);
    expect(onModelsSpy).toHaveBeenLastCalledWith([
      { id: "m2", name: "M2" },
      { id: "m1", name: "M1" },
    ]);
  });
});
