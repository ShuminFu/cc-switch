import type { ReactNode } from "react";
import {
  act,
  render,
  renderHook,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import { Toaster, toast } from "sonner";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { IpcError } from "@/lib/api/ipc";
import { getIpcErrorAction } from "@/lib/errors/errorActions";
import { useSwitchProviderMutation } from "@/lib/query/mutations";
import { createTestQueryClient } from "../utils/testQueryClient";

const t = ((key: string, opts?: Record<string, string>) =>
  opts ? `${key} ${JSON.stringify(opts)}` : key) as unknown as TFunction;

const ipcError = (code: string, details: unknown = null) =>
  new IpcError({
    code: code as never,
    message: `${code} message`,
    details,
    i18nKey: null,
  });

/** 用真实 Toaster 渲染带修复按钮的错误 toast，点击后返回 invoke 调用记录 */
const renderAndClick = async (error: unknown, label: string) => {
  render(<Toaster />);
  const action = getIpcErrorAction(error, t);
  expect(action).not.toBeNull();
  act(() => {
    toast.error("boom", { action: action ?? undefined });
  });
  const button = await screen.findByRole("button", { name: label });
  fireEvent.click(button);
};

describe("getIpcErrorAction", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    toast.dismiss();
  });

  it("TOOL_NOT_INSTALLED offers Install and runs the install lifecycle", async () => {
    await renderAndClick(
      ipcError("TOOL_NOT_INSTALLED", { tool: "codex" }),
      "errors.actions.install",
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("run_tool_lifecycle_action", {
        tools: ["codex"],
        action: "install",
        wslShellByTool: undefined,
      }),
    );
  });

  it("CIRCUIT_OPEN offers Reset and resets the named provider", async () => {
    await renderAndClick(
      ipcError("CIRCUIT_OPEN", { appType: "claude", providerId: "p1" }),
      "errors.actions.resetCircuit",
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("reset_circuit_breaker", {
        providerId: "p1",
        appType: "claude",
      }),
    );
  });

  it("CIRCUIT_OPEN without a provider resets every queued provider", async () => {
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "get_failover_queue"
        ? [
            { providerId: "a", providerName: "A" },
            { providerId: "b", providerName: "B" },
          ]
        : undefined,
    );
    await renderAndClick(
      ipcError("CIRCUIT_OPEN", { appType: "codex" }),
      "errors.actions.resetCircuit",
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("reset_circuit_breaker", {
        providerId: "b",
        appType: "codex",
      }),
    );
    expect(invokeMock).toHaveBeenCalledWith("get_failover_queue", {
      appType: "codex",
    });
    expect(invokeMock).toHaveBeenCalledWith("reset_circuit_breaker", {
      providerId: "a",
      appType: "codex",
    });
  });

  it("TAKEOVER_ACTIVE offers Stop takeover and disables the app takeover", async () => {
    await renderAndClick(
      ipcError("TAKEOVER_ACTIVE", { appType: "gemini" }),
      "errors.actions.stopTakeover",
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
        appType: "gemini",
        enabled: false,
      }),
    );
  });

  it("DB_VERSION_TOO_NEW offers Restore backup and restores the given backup", async () => {
    await renderAndClick(
      ipcError("DB_VERSION_TOO_NEW", { backupId: "db_backup_20260101" }),
      "errors.actions.restoreBackup",
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("restore_db_backup", {
        filename: "db_backup_20260101.db",
      }),
    );
  });

  it("DB_VERSION_TOO_NEW without a backup id restores the newest backup", async () => {
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "list_db_backups"
        ? [
            { filename: "newest.db", sizeBytes: 1, createdAt: "2" },
            { filename: "older.db", sizeBytes: 1, createdAt: "1" },
          ]
        : "safety",
    );
    await renderAndClick(
      ipcError("DB_VERSION_TOO_NEW"),
      "errors.actions.restoreBackup",
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("restore_db_backup", {
        filename: "newest.db",
      }),
    );
  });

  it("reports a failed action in a follow-up toast", async () => {
    invokeMock.mockRejectedValue({
      code: "UNKNOWN",
      message: "proxy down",
      details: null,
      i18nKey: null,
    });
    await renderAndClick(
      ipcError("TAKEOVER_ACTIVE", { appType: "claude" }),
      "errors.actions.stopTakeover",
    );
    expect(
      await screen.findByText(/errors\.actions\.failed.*proxy down/),
    ).toBeInTheDocument();
  });

  it("returns null for unmapped codes, missing context and legacy errors", () => {
    expect(getIpcErrorAction(ipcError("CONFIG"), t)).toBeNull();
    expect(getIpcErrorAction(ipcError("TOOL_NOT_INSTALLED"), t)).toBeNull();
    expect(getIpcErrorAction(ipcError("CIRCUIT_OPEN"), t)).toBeNull();
    expect(getIpcErrorAction(ipcError("TAKEOVER_ACTIVE", {}), t)).toBeNull();
    expect(getIpcErrorAction("legacy string error", t)).toBeNull();
    expect(getIpcErrorAction(new Error("plain"), t)).toBeNull();
  });

  it("accepts a raw IpcError payload as well as the class instance", () => {
    const action = getIpcErrorAction(
      {
        code: "TAKEOVER_ACTIVE",
        message: "m",
        details: { appType: "claude" },
        i18nKey: null,
      },
      t,
    );
    expect(action?.label).toBe("errors.actions.stopTakeover");
  });
});

describe("switch provider error toast", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    toast.dismiss();
  });

  it("offers the code action instead of Copy when switching hits a takeover conflict", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "switch_provider") {
        throw {
          code: "TAKEOVER_ACTIVE",
          message: "takeover active",
          details: { appType: "claude" },
          i18nKey: null,
        };
      }
      return undefined;
    });
    render(<Toaster />);
    const client = createTestQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useSwitchProviderMutation(), {
      wrapper,
    });

    act(() => {
      result.current.mutate({ appId: "claude", providerId: "p1" });
    });

    const button = await screen.findByRole("button", {
      name: "errors.actions.stopTakeover",
    });
    expect(
      screen.queryByRole("button", { name: "common.copy" }),
    ).not.toBeInTheDocument();
    fireEvent.click(button);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
        appType: "claude",
        enabled: false,
      }),
    );
  });
});
