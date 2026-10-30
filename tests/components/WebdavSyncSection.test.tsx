import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { WebdavSyncSection } from "@/components/settings/WebdavSyncSection";
import type {
  RemoteSnapshotInfo,
  S3SyncSettings,
  WebDavSyncSettings,
} from "@/types";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastWarningMock = vi.fn();
const toastInfoMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
  },
}));

const { translateMock } = vi.hoisted(() => ({
  translateMock: vi.fn((key: string, _options?: unknown) => key),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: unknown) => translateMock(key, options),
  }),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => (
    <button {...props}>{children}</button>
  ),
}));

vi.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

vi.mock("@/components/ui/switch", () => ({
  Switch: ({ checked, onCheckedChange, ...props }: any) => (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onCheckedChange?.(!checked)}
      {...props}
    />
  ),
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ value, onValueChange, children }: any) => (
    <select
      data-testid="webdav-preset-select"
      value={value}
      onChange={(event) => onValueChange?.(event.target.value)}
    >
      {children}
    </select>
  ),
  SelectTrigger: ({ children }: any) => <>{children}</>,
  SelectValue: () => null,
  SelectContent: ({ children }: any) => <>{children}</>,
  SelectItem: ({ value, children }: any) => (
    <option value={value}>{children}</option>
  ),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) => (open ? <div>{children}</div> : null),
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogDescription: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
}));

const { settingsApiMock, invokeMock } = vi.hoisted(() => ({
  settingsApiMock: {
    webdavTestConnection: vi.fn(),
    webdavSyncSaveSettings: vi.fn(),
    webdavSyncFetchRemoteInfo: vi.fn(),
    webdavSyncUpload: vi.fn(),
    webdavSyncDownload: vi.fn(),
    s3SyncFetchRemoteInfo: vi.fn(),
    s3SyncDownload: vi.fn(),
  },
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// 同步密钥字段走真实的 settingsApi 包装，断言实际发出的 invoke 命令与参数
vi.mock("@/lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/api/settings")>(
      "@/lib/api/settings",
    );
  return {
    settingsApi: {
      ...settingsApiMock,
      getSyncSnapshotKeyStatus: actual.settingsApi.getSyncSnapshotKeyStatus,
      setSyncSnapshotKey: actual.settingsApi.setSyncSnapshotKey,
    },
  };
});

let syncKeyStatus: { configured: boolean; keyId: string | null };

const baseConfig: WebDavSyncSettings = {
  enabled: true,
  baseUrl: "https://dav.example.com/dav/",
  username: "alice",
  password: "secret",
  remoteRoot: "cc-switch-sync",
  profile: "default",
  autoSync: false,
  status: {},
};

const baseS3Config: S3SyncSettings = {
  enabled: true,
  region: "us-east-1",
  bucket: "my-bucket",
  accessKeyId: "AKIA",
  secretAccessKey: "secret",
  remoteRoot: "cc-switch-sync",
  profile: "default",
  autoSync: false,
  status: {},
};

function renderSection(config?: WebDavSyncSettings, s3Config?: S3SyncSettings) {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <WebdavSyncSection config={config} s3Config={s3Config} />
    </QueryClientProvider>,
  );
  return { ...view, client };
}

describe("WebdavSyncSection", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastWarningMock.mockReset();
    toastInfoMock.mockReset();
    translateMock.mockClear();
    settingsApiMock.webdavTestConnection.mockReset();
    settingsApiMock.webdavSyncSaveSettings.mockReset();
    settingsApiMock.webdavSyncFetchRemoteInfo.mockReset();
    settingsApiMock.webdavSyncUpload.mockReset();
    settingsApiMock.webdavSyncDownload.mockReset();

    settingsApiMock.webdavSyncSaveSettings.mockResolvedValue({ success: true });
    settingsApiMock.webdavTestConnection.mockResolvedValue({
      success: true,
      message: "ok",
    });
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValue({
      deviceName: "My MacBook",
      createdAt: "2026-02-01T10:00:00Z",
      snapshotId: "snapshot-1",
      version: 2,
      compatible: true,
      artifacts: ["db.sql", "skills.zip"],
    });
    settingsApiMock.webdavSyncUpload.mockResolvedValue({ status: "uploaded" });
    settingsApiMock.webdavSyncDownload.mockResolvedValue({
      status: "downloaded",
    });

    syncKeyStatus = { configured: false, keyId: null };
    invokeMock.mockReset();
    invokeMock.mockImplementation(
      async (command: string, args?: { key?: string | null }) => {
        if (command === "get_sync_snapshot_key_status") return syncKeyStatus;
        if (command === "set_sync_snapshot_key") {
          syncKeyStatus = args?.key
            ? { configured: true, keyId: "a1b2c3d4" }
            : { configured: false, keyId: null };
          return syncKeyStatus;
        }
        throw new Error(`unexpected invoke: ${command}`);
      },
    );
  });

  it("shows auto sync error callout when last auto sync failed", () => {
    renderSection({
      ...baseConfig,
      status: {
        lastError: "network timeout",
        lastErrorSource: "auto",
      },
    });

    expect(
      screen.getByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).toBeInTheDocument();
    expect(screen.getByText("network timeout")).toBeInTheDocument();
  });

  it("does not show auto sync error callout for manual sync errors", () => {
    renderSection({
      ...baseConfig,
      status: {
        lastError: "manual upload failed",
        lastErrorSource: "manual",
      },
    });

    expect(
      screen.queryByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).not.toBeInTheDocument();
  });

  it("does not show auto sync error callout when source is missing", () => {
    renderSection({
      ...baseConfig,
      autoSync: true,
      status: {
        lastError: "legacy error without source",
      },
    });

    expect(
      screen.queryByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).not.toBeInTheDocument();
  });

  it("shows validation error when saving without base url", async () => {
    renderSection({ ...baseConfig, baseUrl: "" });

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );

    expect(toastErrorMock).toHaveBeenCalledWith(
      "settings.webdavSync.missingUrl",
    );
    expect(settingsApiMock.webdavSyncSaveSettings).not.toHaveBeenCalled();
  });

  it("saves settings and auto tests connection", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });
    expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        baseUrl: "https://dav.example.com/dav/",
        username: "alice",
        password: "",
        autoSync: false,
      }),
      false,
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavTestConnection).toHaveBeenCalledWith(
        expect.objectContaining({
          baseUrl: "https://dav.example.com/dav/",
        }),
        true,
      );
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.saveAndTestSuccess",
    );
  });

  it("preserves password only for the single post-save refresh", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("secret");

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("");
  });

  it("does not preserve password after a later external config refresh", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("secret");

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection
          config={{ ...baseConfig, username: "bob", password: "" }}
        />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("");
  });

  it("does not submit a preserved password again when testing without touching it", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.test" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavTestConnection).toHaveBeenLastCalledWith(
        expect.objectContaining({
          password: "",
        }),
        true,
      );
    });
  });

  it("saves auto sync as true after toggle", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("switch", { name: "settings.webdavSync.autoSync" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "confirm.autoSync.confirm" }),
    );
    await waitFor(() => {
      expect(
        screen.getByRole("switch", { name: "settings.webdavSync.autoSync" }),
      ).toHaveAttribute("aria-checked", "true");
    });
    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          autoSync: true,
        }),
        false,
      );
    });
  });

  it("blocks upload when there are unsaved changes", async () => {
    renderSection(baseConfig);

    fireEvent.change(
      screen.getByPlaceholderText("settings.webdavSync.usernamePlaceholder"),
      { target: { value: "bob" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );

    expect(toastErrorMock).toHaveBeenCalledWith(
      "settings.webdavSync.unsavedChanges",
    );
    expect(settingsApiMock.webdavSyncFetchRemoteInfo).not.toHaveBeenCalled();
  });

  it("disables sync buttons until config is saved", () => {
    renderSection(undefined);

    const uploadButton = screen.getByRole("button", {
      name: "settings.webdavSync.upload",
    });
    const downloadButton = screen.getByRole("button", {
      name: "settings.webdavSync.download",
    });

    expect(uploadButton).toBeDisabled();
    expect(downloadButton).toBeDisabled();
    expect(settingsApiMock.webdavSyncFetchRemoteInfo).not.toHaveBeenCalled();
  });

  it("fetches remote info and uploads after confirmation", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(
        1,
      );
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmUpload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncUpload).toHaveBeenCalledTimes(1);
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.uploadSuccess",
    );
  });

  it("blocks upload confirmation if form changes after dialog opens", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(
        1,
      );
    });

    fireEvent.change(screen.getByPlaceholderText("cc-switch-sync"), {
      target: { value: "new-root" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmUpload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.unsavedChanges",
      );
    });
    expect(settingsApiMock.webdavSyncUpload).not.toHaveBeenCalled();
  });

  it("fetches remote info and downloads after confirmation", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(
        1,
      );
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncDownload).toHaveBeenCalledTimes(1);
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.downloadSuccess",
    );
  });

  it("blocks download confirmation if form changes after dialog opens", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(
        1,
      );
    });

    fireEvent.change(screen.getByPlaceholderText("default"), {
      target: { value: "new-profile" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.unsavedChanges",
      );
    });
    expect(settingsApiMock.webdavSyncDownload).not.toHaveBeenCalled();
  });

  it("shows info when no remote snapshot is found for download", async () => {
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValueOnce({
      empty: true,
    });
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(toastInfoMock).toHaveBeenCalledWith(
        "settings.webdavSync.noRemoteData",
      );
    });
    expect(settingsApiMock.webdavSyncDownload).not.toHaveBeenCalled();
  });

  it("blocks download when remote snapshot is incompatible", async () => {
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValueOnce({
      deviceName: "Legacy Machine",
      createdAt: "2025-01-01T00:00:00Z",
      snapshotId: "legacy-snapshot",
      version: 1,
      compatible: false,
      artifacts: ["db.sql"],
    });
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.incompatibleVersion",
      );
    });
    expect(settingsApiMock.webdavSyncDownload).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    ).not.toBeInTheDocument();
  });

  it("shows error when download fails after confirmation", async () => {
    settingsApiMock.webdavSyncDownload.mockRejectedValueOnce(new Error("boom"));
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(
        1,
      );
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.downloadFailed",
      );
    });
  });

  describe("sync snapshot key field (CCS-237)", () => {
    it("renders the not-set status", async () => {
      renderSection(baseConfig);

      expect(
        await screen.findByText("settings.webdavSync.syncKey.statusNotSet"),
      ).toBeInTheDocument();
      expect(invokeMock).toHaveBeenCalledWith("get_sync_snapshot_key_status");
      expect(
        screen.getByRole("button", {
          name: "settings.webdavSync.syncKey.clear",
        }),
      ).toBeDisabled();
    });

    it("renders the configured status with the key id", async () => {
      syncKeyStatus = { configured: true, keyId: "deadbeef" };
      renderSection(baseConfig);

      expect(
        await screen.findByText("settings.webdavSync.syncKey.statusConfigured"),
      ).toBeInTheDocument();
      expect(screen.getByText("deadbeef")).toBeInTheDocument();
    });

    it("rejects keys shorter than 8 characters without invoking", async () => {
      renderSection(baseConfig);
      await screen.findByText("settings.webdavSync.syncKey.statusNotSet");

      fireEvent.change(
        screen.getByLabelText("settings.webdavSync.syncKey.title"),
        { target: { value: "short" } },
      );

      expect(
        screen.getByText("settings.webdavSync.syncKey.tooShort"),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "settings.webdavSync.syncKey.set" }),
      ).toBeDisabled();
      expect(invokeMock).not.toHaveBeenCalledWith(
        "set_sync_snapshot_key",
        expect.anything(),
      );
    });

    it("sets the key via set_sync_snapshot_key", async () => {
      renderSection(baseConfig);
      await screen.findByText("settings.webdavSync.syncKey.statusNotSet");

      fireEvent.change(
        screen.getByLabelText("settings.webdavSync.syncKey.title"),
        { target: { value: "correct horse battery" } },
      );
      fireEvent.click(
        screen.getByRole("button", { name: "settings.webdavSync.syncKey.set" }),
      );

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith("set_sync_snapshot_key", {
          key: "correct horse battery",
        });
      });
      expect(await screen.findByText("a1b2c3d4")).toBeInTheDocument();
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "settings.webdavSync.syncKey.setSuccess",
      );
      expect(
        screen.getByLabelText("settings.webdavSync.syncKey.title"),
      ).toHaveValue("");
    });

    it("clears the key with null after confirmation", async () => {
      syncKeyStatus = { configured: true, keyId: "deadbeef" };
      renderSection(baseConfig);
      await screen.findByText("deadbeef");

      fireEvent.click(
        screen.getByRole("button", {
          name: "settings.webdavSync.syncKey.clear",
        }),
      );
      expect(
        screen.getByText("settings.webdavSync.syncKey.clearConfirm.message"),
      ).toBeInTheDocument();
      expect(invokeMock).not.toHaveBeenCalledWith(
        "set_sync_snapshot_key",
        expect.anything(),
      );

      fireEvent.click(
        screen.getByRole("button", {
          name: "settings.webdavSync.syncKey.clearConfirm.confirm",
        }),
      );

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith("set_sync_snapshot_key", {
          key: null,
        });
      });
      expect(
        await screen.findByText("settings.webdavSync.syncKey.statusNotSet"),
      ).toBeInTheDocument();
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "settings.webdavSync.syncKey.clearSuccess",
      );
    });
  });
  describe("remote snapshot signature (CCS-238)", () => {
    const remoteInfo = (
      signature: RemoteSnapshotInfo["signature"],
    ): RemoteSnapshotInfo => ({
      deviceName: "Old Laptop",
      createdAt: "2026-02-01T10:00:00Z",
      snapshotId: "snapshot-1",
      version: 2,
      protocolVersion: 2,
      dbCompatVersion: 6,
      compatible: true,
      signature,
      artifacts: ["db.sql", "skills.zip"],
      layout: "current",
      remotePath: "/cc-switch-sync/v2/db-v6/default",
    });

    const backends = [
      {
        name: "webdav",
        render: (signedSync = false) =>
          renderSection({ ...baseConfig, status: { signedSync } }),
        fetchInfo: settingsApiMock.webdavSyncFetchRemoteInfo,
        downloadMock: settingsApiMock.webdavSyncDownload,
        downloadCommand: "webdav_sync_download",
        downloadButton: "settings.webdavSync.download",
        confirmButton: "settings.webdavSync.confirmDownload.confirm",
        downloadFailed: "settings.webdavSync.downloadFailed",
      },
      {
        name: "s3",
        render: (signedSync = false) =>
          renderSection(baseConfig, {
            ...baseS3Config,
            status: { signedSync },
          }),
        fetchInfo: settingsApiMock.s3SyncFetchRemoteInfo,
        downloadMock: settingsApiMock.s3SyncDownload,
        downloadCommand: "s3_sync_download",
        downloadButton: "settings.s3Sync.download",
        confirmButton: "settings.s3Sync.confirmDownload.confirm",
        downloadFailed: "settings.s3Sync.downloadFailed",
      },
    ] as const;

    let downloadError: unknown;

    beforeEach(async () => {
      const actual =
        await vi.importActual<typeof import("@/lib/api/settings")>(
          "@/lib/api/settings",
        );
      // 下载走真实的 settingsApi 包装，断言实际发出的 invoke 命令与参数
      settingsApiMock.webdavSyncDownload.mockImplementation(
        actual.settingsApi.webdavSyncDownload,
      );
      settingsApiMock.s3SyncFetchRemoteInfo.mockReset();
      settingsApiMock.s3SyncDownload.mockReset();
      settingsApiMock.s3SyncDownload.mockImplementation(
        actual.settingsApi.s3SyncDownload,
      );
      downloadError = undefined;

      const baseInvoke = invokeMock.getMockImplementation()!;
      invokeMock.mockImplementation(async (command: string, args?: any) => {
        if (
          command === "webdav_sync_download" ||
          command === "s3_sync_download"
        ) {
          if (downloadError) throw downloadError;
          return { status: "downloaded", signed: false };
        }
        return baseInvoke(command, args);
      });
    });

    describe.each(backends)("$name", (backend) => {
      const clickDownload = () =>
        fireEvent.click(
          screen.getByRole("button", { name: backend.downloadButton }),
        );
      const expectNoDownload = () => {
        expect(invokeMock).not.toHaveBeenCalledWith(
          backend.downloadCommand,
          expect.anything(),
        );
        expect(
          screen.queryByRole("button", { name: backend.confirmButton }),
        ).not.toBeInTheDocument();
        expect(
          screen.queryByRole("button", {
            name: "settings.webdavSync.signature.migrate.confirm",
          }),
        ).not.toBeInTheDocument();
      };

      it("downloads a valid signed snapshot normally", async () => {
        syncKeyStatus = { configured: true, keyId: "a1b2c3d4" };
        backend.fetchInfo.mockResolvedValueOnce(remoteInfo("valid"));
        backend.render();

        clickDownload();
        fireEvent.click(
          await screen.findByRole("button", { name: backend.confirmButton }),
        );

        await waitFor(() => {
          expect(invokeMock).toHaveBeenCalledWith(backend.downloadCommand, {
            allowUnsigned: false,
          });
        });
        expect(
          screen.queryByText("settings.webdavSync.signature.unsignedHint"),
        ).not.toBeInTheDocument();
      });

      it("blocks a tampered snapshot with an error toast", async () => {
        syncKeyStatus = { configured: true, keyId: "a1b2c3d4" };
        backend.fetchInfo.mockResolvedValueOnce(remoteInfo("invalid"));
        backend.render();

        clickDownload();

        await waitFor(() => {
          expect(toastErrorMock).toHaveBeenCalledWith(
            "settings.webdavSync.signature.invalid",
          );
        });
        expectNoDownload();
      });

      it("asks for the sync key when the snapshot is signed but no key is set", async () => {
        backend.fetchInfo.mockResolvedValueOnce(remoteInfo("keyMissing"));
        backend.render();

        clickDownload();

        await waitFor(() => {
          expect(toastWarningMock).toHaveBeenCalledWith(
            "settings.webdavSync.signature.keyMissing",
          );
        });
        expectNoDownload();
      });

      it("shows the unsigned hint in the normal dialog when no key is set", async () => {
        backend.fetchInfo.mockResolvedValueOnce(remoteInfo("unsigned"));
        backend.render();

        clickDownload();

        expect(
          await screen.findByText("settings.webdavSync.signature.unsignedHint"),
        ).toBeInTheDocument();
        fireEvent.click(
          screen.getByRole("button", { name: backend.confirmButton }),
        );
        await waitFor(() => {
          expect(invokeMock).toHaveBeenCalledWith(backend.downloadCommand, {
            allowUnsigned: false,
          });
        });
      });

      it("offers a one-time migration for an unsigned snapshot once a key is set", async () => {
        syncKeyStatus = { configured: true, keyId: "a1b2c3d4" };
        backend.fetchInfo.mockResolvedValueOnce(remoteInfo("unsigned"));
        backend.render();

        clickDownload();

        expect(
          await screen.findByText(
            "settings.webdavSync.signature.migrate.message",
          ),
        ).toBeInTheDocument();
        expect(
          screen.getByText("settings.webdavSync.signature.migrate.title"),
        ).toBeInTheDocument();
        expect(
          screen.queryByRole("button", { name: backend.confirmButton }),
        ).not.toBeInTheDocument();

        fireEvent.click(
          screen.getByRole("button", {
            name: "settings.webdavSync.signature.migrate.confirm",
          }),
        );

        await waitFor(() => {
          expect(invokeMock).toHaveBeenCalledWith(backend.downloadCommand, {
            allowUnsigned: true,
          });
        });
        expect(toastSuccessMock).toHaveBeenCalled();
      });

      it("refuses an unsigned snapshot after a signed sync (downgrade)", async () => {
        syncKeyStatus = { configured: true, keyId: "a1b2c3d4" };
        backend.fetchInfo.mockResolvedValueOnce(remoteInfo("unsigned"));
        backend.render(true);

        clickDownload();

        await waitFor(() => {
          expect(toastErrorMock).toHaveBeenCalledWith(
            "settings.webdavSync.signature.errors.manifestUnsignedRejected",
          );
        });
        expectNoDownload();
      });

      it("localizes sync.* errors returned by the download command", async () => {
        syncKeyStatus = { configured: true, keyId: "a1b2c3d4" };
        backend.fetchInfo.mockResolvedValueOnce(remoteInfo("valid"));
        downloadError = {
          code: "LOCALIZED",
          message: "远端 manifest 签名校验失败",
          details: { zh: "远端 manifest 签名校验失败", en: "invalid" },
          i18nKey: "sync.manifest_signature_invalid",
        };
        backend.render();

        clickDownload();
        fireEvent.click(
          await screen.findByRole("button", { name: backend.confirmButton }),
        );

        await waitFor(() => {
          expect(toastErrorMock).toHaveBeenCalledWith(backend.downloadFailed);
        });
        expect(translateMock).toHaveBeenCalledWith(backend.downloadFailed, {
          error:
            "settings.webdavSync.signature.errors.manifestSignatureInvalid",
        });
      });
    });
  });
});
