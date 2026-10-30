import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { CloudSyncPanel } from "@/components/settings/cloudSync/CloudSyncPanel";
import { s3Adapter } from "@/components/settings/cloudSync/s3Adapter";
import {
  detectWebdavPreset,
  webdavAdapter,
} from "@/components/settings/cloudSync/webdavAdapter";
import { detectSyncConflict } from "@/lib/errors/syncConflict";
import { BackendError } from "@/lib/errors/appError";
import type { S3SyncSettings, WebDavSyncSettings } from "@/types";

const toast = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));

vi.mock("sonner", () => ({ toast }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
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
      data-testid="preset-select"
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

const api = vi.hoisted(() => ({
  webdavTestConnection: vi.fn(),
  webdavSyncSaveSettings: vi.fn(),
  webdavSyncFetchRemoteInfo: vi.fn(),
  webdavSyncUpload: vi.fn(),
  webdavSyncDownload: vi.fn(),
  s3TestConnection: vi.fn(),
  s3SyncSaveSettings: vi.fn(),
  s3SyncFetchRemoteInfo: vi.fn(),
  s3SyncUpload: vi.fn(),
  s3SyncDownload: vi.fn(),
}));

vi.mock("@/lib/api", () => ({ settingsApi: api }));

// The sync commands reject with a structured AppError (`{ code, message, params }`);
// the API layer turns it into a BackendError carrying the code.
const REMOTE_CHANGED = new BackendError({
  code: "sync.conflict.remote_changed",
  message:
    '同步冲突：远端数据自上次同步后已被其他设备更新（来自设备「Laptop」）。 (Sync conflict: the remote data changed since the last sync (from device "Laptop" at 2026-02-01). Upload was cancelled to avoid overwriting it.)',
});
// A raw payload (not yet wrapped) is recognised as well.
const LOCAL_CHANGED = {
  code: "sync.conflict.local_changed",
  message:
    "同步冲突：本地数据自上次同步后已修改，下载将覆盖这些修改。 (Sync conflict: local data changed since the last sync and downloading would overwrite it.)",
};

interface BackendCase {
  name: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  adapter: any;
  config: WebDavSyncSettings | S3SyncSettings;
  prefix: string;
  secretPlaceholder: string;
  editPlaceholder: string;
  scopeField: string;
  calls: {
    test: keyof typeof api;
    save: keyof typeof api;
    fetch: keyof typeof api;
    upload: keyof typeof api;
    download: keyof typeof api;
  };
}

const cases: BackendCase[] = [
  {
    name: "webdav",
    adapter: webdavAdapter,
    config: {
      enabled: true,
      baseUrl: "https://dav.example.com/dav/",
      username: "alice",
      password: "secret",
      remoteRoot: "cc-switch-sync",
      profile: "default",
      autoSync: false,
      status: {},
    },
    prefix: "settings.webdavSync",
    secretPlaceholder: "settings.webdavSync.passwordPlaceholder",
    editPlaceholder: "settings.webdavSync.usernamePlaceholder",
    scopeField: "username",
    calls: {
      test: "webdavTestConnection",
      save: "webdavSyncSaveSettings",
      fetch: "webdavSyncFetchRemoteInfo",
      upload: "webdavSyncUpload",
      download: "webdavSyncDownload",
    },
  },
  {
    name: "s3",
    adapter: s3Adapter,
    config: {
      enabled: true,
      region: "us-east-1",
      bucket: "bucket-a",
      accessKeyId: "AKIA1",
      secretAccessKey: "secret",
      remoteRoot: "cc-switch-sync",
      profile: "default",
      autoSync: false,
      status: {},
    },
    prefix: "settings.s3Sync",
    secretPlaceholder: "settings.s3Sync.secretAccessKeyPlaceholder",
    editPlaceholder: "settings.s3Sync.bucketPlaceholder",
    scopeField: "bucket",
    calls: {
      test: "s3TestConnection",
      save: "s3SyncSaveSettings",
      fetch: "s3SyncFetchRemoteInfo",
      upload: "s3SyncUpload",
      download: "s3SyncDownload",
    },
  },
];

const remoteInfo = {
  deviceName: "Laptop",
  createdAt: "2026-02-01T10:00:00Z",
  snapshotId: "snap-1",
  version: 2,
  protocolVersion: 2,
  dbCompatVersion: 6,
  compatible: true,
  artifacts: ["db.sql", "skills.zip"],
  layout: "current",
  remotePath: "/cc-switch-sync/v2/db-v6/default",
};

describe.each(cases)("CloudSyncPanel parity: $name", (c) => {
  const renderPanel = (
    config = c.config,
    options: { autoSyncConfirmed?: boolean } = {},
  ) => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const ui = (cfg: typeof config) => (
      <QueryClientProvider client={client}>
        <CloudSyncPanel adapter={c.adapter} config={cfg} {...options} />
      </QueryClientProvider>
    );
    const view = render(ui(config));
    return {
      ...view,
      rerenderWith: (cfg: typeof config) => view.rerender(ui(cfg)),
    };
  };
  const click = (name: string) =>
    fireEvent.click(screen.getByRole("button", { name }));
  const secretInput = () =>
    screen.getByPlaceholderText(c.secretPlaceholder) as HTMLInputElement;

  beforeEach(() => {
    Object.values(toast).forEach((fn) => fn.mockReset());
    Object.values(api).forEach((fn) => fn.mockReset());
    api[c.calls.save].mockResolvedValue({ success: true });
    api[c.calls.test].mockResolvedValue({ success: true });
    api[c.calls.fetch].mockResolvedValue(remoteInfo);
    api[c.calls.upload].mockResolvedValue({ status: "uploaded" });
    api[c.calls.download].mockResolvedValue({ status: "downloaded" });
  });

  it("saves with an empty untouched secret and auto-tests preserving it", async () => {
    renderPanel();
    click(`${c.prefix}.save`);
    await waitFor(() => expect(api[c.calls.test]).toHaveBeenCalled());
    const [saved, touched] = api[c.calls.save].mock.calls[0];
    expect(Object.values(saved)).toContain("");
    expect(touched).toBe(false);
    expect(api[c.calls.test]).toHaveBeenCalledWith(expect.anything(), true);
    expect(toast.success).toHaveBeenCalledWith(
      `${c.prefix}.saveAndTestSuccess`,
    );
  });

  it("keeps the typed secret for the single redacted refetch after a save", async () => {
    const view = renderPanel();
    click(`${c.prefix}.save`);
    await waitFor(() => expect(api[c.calls.save]).toHaveBeenCalled());

    const redacted = { ...c.config, password: "", secretAccessKey: "" };
    view.rerenderWith(redacted);
    expect(secretInput().value).toBe("secret");

    view.rerenderWith({ ...redacted, [c.scopeField]: "someone-else" });
    expect(secretInput().value).toBe("");
  });

  it("blocks upload while there are unsaved changes", () => {
    renderPanel();
    fireEvent.change(screen.getByPlaceholderText(c.editPlaceholder), {
      target: { value: "changed" },
    });
    click(`${c.prefix}.upload`);
    expect(toast.error).toHaveBeenCalledWith(`${c.prefix}.unsavedChanges`);
    expect(api[c.calls.fetch]).not.toHaveBeenCalled();
  });

  it("uploads without force after the preview is confirmed", async () => {
    renderPanel();
    click(`${c.prefix}.upload`);
    await waitFor(() => expect(api[c.calls.fetch]).toHaveBeenCalled());
    click(`${c.prefix}.confirmUpload.confirm`);
    await waitFor(() =>
      expect(api[c.calls.upload]).toHaveBeenCalledWith({ force: false }),
    );
    expect(toast.success).toHaveBeenCalledWith(`${c.prefix}.uploadSuccess`);
  });

  it("asks before overwriting a remote that changed, then forces the upload", async () => {
    api[c.calls.fetch].mockResolvedValue({
      ...remoteInfo,
      remoteChangedSinceLastSync: true,
    });
    api[c.calls.upload].mockRejectedValueOnce(REMOTE_CHANGED);
    renderPanel();
    click(`${c.prefix}.upload`);
    await waitFor(() => expect(api[c.calls.fetch]).toHaveBeenCalled());
    expect(
      screen.getByText("settings.cloudSync.remoteChangedNotice"),
    ).toBeInTheDocument();

    click(`${c.prefix}.confirmUpload.confirm`);
    await waitFor(() =>
      expect(
        screen.getByText("settings.cloudSync.conflict.remoteChanged"),
      ).toBeInTheDocument(),
    );
    expect(toast.error).not.toHaveBeenCalled();

    click("settings.cloudSync.conflict.overwriteRemote");
    await waitFor(() =>
      expect(api[c.calls.upload]).toHaveBeenLastCalledWith({ force: true }),
    );
    expect(api[c.calls.upload]).toHaveBeenCalledTimes(2);
    expect(toast.success).toHaveBeenCalledWith(`${c.prefix}.uploadSuccess`);
  });

  it("asks before a download overwrites changed local data", async () => {
    api[c.calls.download].mockRejectedValueOnce(
      new BackendError(LOCAL_CHANGED),
    );
    renderPanel();
    click(`${c.prefix}.download`);
    await waitFor(() => expect(api[c.calls.fetch]).toHaveBeenCalled());
    click(`${c.prefix}.confirmDownload.confirm`);
    await waitFor(() =>
      expect(
        screen.getByText("settings.cloudSync.conflict.localChanged"),
      ).toBeInTheDocument(),
    );

    // Cancelling keeps local data: no forced retry.
    click("common.cancel");
    expect(api[c.calls.download]).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByText("settings.cloudSync.conflict.localChanged"),
    ).not.toBeInTheDocument();
  });

  it("forces the download after the user chooses to overwrite local data", async () => {
    api[c.calls.download].mockRejectedValueOnce(LOCAL_CHANGED);
    renderPanel();
    click(`${c.prefix}.download`);
    await waitFor(() => expect(api[c.calls.fetch]).toHaveBeenCalled());
    click(`${c.prefix}.confirmDownload.confirm`);
    await waitFor(() =>
      screen.getByText("settings.cloudSync.conflict.localChanged"),
    );
    click("settings.cloudSync.conflict.overwriteLocal");
    await waitFor(() =>
      expect(api[c.calls.download]).toHaveBeenLastCalledWith({ force: true }),
    );
    expect(toast.success).toHaveBeenCalledWith(`${c.prefix}.downloadSuccess`);
  });

  it("reports other transfer errors without a conflict dialog", async () => {
    api[c.calls.download].mockRejectedValueOnce(new Error("boom"));
    renderPanel();
    click(`${c.prefix}.download`);
    await waitFor(() => expect(api[c.calls.fetch]).toHaveBeenCalled());
    click(`${c.prefix}.confirmDownload.confirm`);
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(`${c.prefix}.downloadFailed`),
    );
    expect(
      screen.queryByText("settings.cloudSync.conflict.title"),
    ).not.toBeInTheDocument();
  });

  it("blocks download of an incompatible remote snapshot", async () => {
    api[c.calls.fetch].mockResolvedValue({ ...remoteInfo, compatible: false });
    renderPanel();
    click(`${c.prefix}.download`);
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        `${c.prefix}.incompatibleVersion`,
      ),
    );
    expect(api[c.calls.download]).not.toHaveBeenCalled();
  });

  it("shows the legacy-layout notice in the download preview", async () => {
    api[c.calls.fetch].mockResolvedValue({ ...remoteInfo, layout: "legacy" });
    renderPanel();
    click(`${c.prefix}.download`);
    await waitFor(() =>
      expect(
        screen.getByText(c.adapter.messages.confirmDownload.legacyNotice),
      ).toBeInTheDocument(),
    );
  });

  it("warns when the endpoint uses plain HTTP", async () => {
    api[c.calls.test].mockResolvedValue({
      success: true,
      insecureTransport: true,
    });
    renderPanel();
    click(`${c.prefix}.test`);
    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith(
        "settings.cloudSync.insecureTransport",
      ),
    );
  });

  it("asks for confirmation before enabling auto sync the first time", async () => {
    renderPanel(c.config, { autoSyncConfirmed: false });
    const toggle = () =>
      screen.getByRole("switch", { name: `${c.prefix}.autoSync` });
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    click("confirm.autoSync.confirm");
    await waitFor(() =>
      expect(toggle()).toHaveAttribute("aria-checked", "true"),
    );
  });
});

describe("cloud sync helpers", () => {
  it("detects Synology only by its WebDAV port", () => {
    expect(detectWebdavPreset("http://nas.local:5005/")).toBe("synology");
    expect(detectWebdavPreset("https://nas.local:5006/dav")).toBe("synology");
    expect(detectWebdavPreset("https://dav.example.com/path:5005x")).toBe(
      "custom",
    );
    expect(detectWebdavPreset("https://example.com:50050/")).toBe("custom");
    expect(detectWebdavPreset("https://dav.jianguoyun.com/dav/")).toBe(
      "jianguoyun",
    );
  });

  it("recognises both conflict kinds and ignores other errors", () => {
    expect(detectSyncConflict(REMOTE_CHANGED)).toBe("remote_changed");
    expect(detectSyncConflict(LOCAL_CHANGED)).toBe("local_changed");
    expect(detectSyncConflict(new BackendError(LOCAL_CHANGED))).toBe(
      "local_changed",
    );
    // Matching is by code, not by message text.
    expect(detectSyncConflict(new Error(LOCAL_CHANGED.message))).toBeNull();
    expect(
      detectSyncConflict({ code: "errors.config", message: "x" }),
    ).toBeNull();
    expect(detectSyncConflict("sync.conflict.remote_changed")).toBe(
      "remote_changed",
    );
    expect(detectSyncConflict("network timeout")).toBeNull();
    expect(detectSyncConflict(undefined)).toBeNull();
  });
});
