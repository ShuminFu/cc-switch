import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import McpWizardModal from "@/components/mcp/McpWizardModal";
import { parseEnvText, parseHeadersText } from "@/components/mcp/wizardParsers";

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, string>) =>
      (options?.defaultValue ?? key).replace(
        /\{\{(\w+)\}\}/g,
        (_, name: string) => options?.[name] ?? "",
      ),
  }),
}));

describe("wizard parsers (FE-UI-32)", () => {
  it("reports lines it cannot parse instead of dropping them", () => {
    expect(parseEnvText("A=1\nbroken\n=x\n")).toEqual({
      values: { A: "1" },
      invalidLines: ["broken", "=x"],
    });
    expect(parseHeadersText("Authorization: Bearer x\nnope")).toEqual({
      values: { Authorization: "Bearer x" },
      invalidLines: ["nope"],
    });
  });
});

describe("McpWizardModal (FE-UI-32)", () => {
  const renderWizard = () => {
    const onApply = vi.fn();
    render(<McpWizardModal isOpen onClose={vi.fn()} onApply={onApply} />);
    fireEvent.change(screen.getByPlaceholderText("mcp.form.titlePlaceholder"), {
      target: { value: "srv" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("mcp.wizard.commandPlaceholder"),
      { target: { value: "npx" } },
    );
    return onApply;
  };

  it("applies with Ctrl+Enter and includes cwd", () => {
    const onApply = renderWizard();
    fireEvent.change(screen.getByLabelText("Working directory"), {
      target: { value: " /work " },
    });
    fireEvent.keyDown(screen.getByLabelText("Working directory"), {
      key: "Enter",
      ctrlKey: true,
    });
    expect(onApply).toHaveBeenCalledTimes(1);
    const [title, json] = onApply.mock.calls[0];
    expect(title).toBe("srv");
    expect(JSON.parse(json)).toMatchObject({ command: "npx", cwd: "/work" });
  });

  it("shows unparsed env lines and refuses to apply", () => {
    const onApply = renderWizard();
    fireEvent.change(screen.getByPlaceholderText("mcp.wizard.envPlaceholder"), {
      target: { value: "GOOD=1\nnot-a-pair" },
    });
    expect(screen.getByRole("alert")).toHaveTextContent("not-a-pair");
    fireEvent.click(screen.getByText("mcp.wizard.apply"));
    expect(onApply).not.toHaveBeenCalled();
  });

  it("masks secret env values in the preview but applies the real value", () => {
    const onApply = renderWizard();
    fireEvent.change(screen.getByPlaceholderText("mcp.wizard.envPlaceholder"), {
      target: { value: "API_KEY=sk-ant-abcdef123456" },
    });
    const preview = screen.getByText("mcp.wizard.preview").nextElementSibling;
    expect(preview?.textContent).toContain("API_KEY");
    expect(preview?.textContent).not.toContain("abcdef123456");
    fireEvent.click(screen.getByText("mcp.wizard.apply"));
    expect(JSON.parse(onApply.mock.calls[0][1]).env).toEqual({
      API_KEY: "sk-ant-abcdef123456",
    });
  });
});
