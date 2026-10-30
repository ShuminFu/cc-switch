import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import { maskEnvValue } from "@/components/env/maskEnvValue";
import type { EnvConflict } from "@/types/env";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

const conflict: EnvConflict = {
  varName: "ANTHROPIC_API_KEY",
  varValue: "sk-ant-abcdef123456",
  sourceType: "file",
  sourcePath: "/home/u/.zshrc",
};

beforeEach(() => {
  toastMock.success.mockReset();
  toastMock.error.mockReset();
});

describe("maskEnvValue", () => {
  it("keeps the prefix and last four characters of secrets", () => {
    expect(maskEnvValue("ANTHROPIC_API_KEY", "sk-ant-abcdef123456")).toBe(
      "sk-ant-…3456",
    );
    expect(maskEnvValue("OPENAI_TOKEN", "short")).not.toContain("short");
    expect(maskEnvValue("ANTHROPIC_BASE_URL", "https://x")).toBe("https://x");
  });
});

describe("EnvWarningBanner", () => {
  it("masks API keys until the reveal toggle is clicked (FE-UI-53)", () => {
    render(
      <EnvWarningBanner
        conflicts={[conflict]}
        onDismiss={vi.fn()}
        onDeleted={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByText("env.actions.expand"));

    expect(document.body.textContent).not.toContain("abcdef123456");
    fireEvent.click(screen.getByRole("button", { name: "Show value" }));
    expect(document.body.textContent).toContain("sk-ant-abcdef123456");
  });

  it("offers Undo after deleting, which restores the backup (FE-UI-54)", async () => {
    const api = mockInvoke({
      delete_env_vars: { backupPath: "/backups/env-1.json" },
      restore_env_backup: null,
    });
    const onDeleted = vi.fn();
    render(
      <EnvWarningBanner
        conflicts={[conflict]}
        onDismiss={vi.fn()}
        onDeleted={onDeleted}
      />,
    );
    fireEvent.click(screen.getByText("env.actions.expand"));
    fireEvent.click(screen.getByLabelText("ANTHROPIC_API_KEY"));
    fireEvent.click(screen.getByText("env.actions.deleteSelected"));
    fireEvent.click(await screen.findByText("env.confirm.confirm"));

    await waitFor(() => expect(toastMock.success).toHaveBeenCalled());
    const options = toastMock.success.mock.calls[0][1] as {
      action: { onClick: () => void };
    };
    options.action.onClick();

    await waitFor(() =>
      expect(api.callsTo("restore_env_backup")).toHaveLength(1),
    );
    expect(api.callsTo("restore_env_backup")[0].args).toEqual({
      backupPath: "/backups/env-1.json",
    });
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(2));
  });
});
