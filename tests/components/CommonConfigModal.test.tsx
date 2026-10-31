import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommonConfigModal } from "@/components/providers/forms/CommonConfigModal";

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    title,
    children,
    footer,
  }: {
    isOpen: boolean;
    title: string;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        <h1>{title}</h1>
        {children}
        {footer}
      </div>
    ) : null,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
    language,
  }: {
    value: string;
    onChange: (v: string) => void;
    language: string;
  }) => (
    <textarea
      aria-label={`editor-${language}`}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
}));

describe("CommonConfigModal (FE-PROV-18)", () => {
  it("draft mode saves through onSave and closes only when accepted", async () => {
    const onSave = vi.fn().mockReturnValueOnce(false).mockReturnValue(true);
    const onClose = vi.fn();
    render(
      <CommonConfigModal
        isOpen
        onClose={onClose}
        title="Codex snippet"
        value=""
        language="toml"
        placeholder=""
        onSave={onSave}
      />,
    );
    expect(screen.getByText("commonConfig.emptyTitle")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("editor-toml"), {
      target: { value: "model = 'x'" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledWith("model = 'x'"));
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("draft mode Cancel discards the draft", () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    render(
      <CommonConfigModal
        isOpen
        onClose={onClose}
        title="Gemini snippet"
        value='{"A":"1"}'
        language="json"
        placeholder=""
        onSave={onSave}
      />,
    );
    fireEvent.change(screen.getByLabelText("editor-json"), {
      target: { value: "{}" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(onSave).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("live mode applies every edit and Save just closes", () => {
    const onChange = vi.fn();
    const onClose = vi.fn();
    render(
      <CommonConfigModal
        isOpen
        onClose={onClose}
        title="Claude snippet"
        value="{}"
        language="json"
        placeholder=""
        onChange={onChange}
        onExtract={vi.fn()}
        extractLabel="Extract"
      />,
    );
    fireEvent.change(screen.getByLabelText("editor-json"), {
      target: { value: '{"env":{}}' },
    });
    expect(onChange).toHaveBeenCalledWith('{"env":{}}');
    expect(screen.getByRole("button", { name: "Extract" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    expect(onClose).toHaveBeenCalled();
  });
});
