// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BackendError } from "@/lib/errors/appError";
import { errorReport, showError } from "@/lib/errors/showError";

const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

const configError = () =>
  new BackendError({
    code: "errors.config",
    message: "配置错误: live config is read-only",
    params: { detail: "live config is read-only" },
  });

describe("showError (DOCS-19)", () => {
  beforeEach(() => {
    toastMock.error.mockReset();
    toastMock.success.mockReset();
  });

  it("shows a localized title with the English detail, not the raw backend text", () => {
    showError({ title: "Failed to save", error: configError() });

    const [title, options] = toastMock.error.mock.calls[0];
    expect(title).toBe("Failed to save");
    expect(options.description).toBe(
      "Configuration error: live config is read-only",
    );
    expect(options.description).not.toMatch(/配置错误/);
    expect(options.action.label).toBe("Copy details");
  });

  it("copies title, code and detail for a bug report", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    showError({ title: "Failed to save", error: configError() });
    toastMock.error.mock.calls[0][1].action.onClick();

    await vi.waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        "Failed to save\nCode: errors.config\nDetail: Configuration error: live config is read-only",
      ),
    );
    expect(toastMock.success).toHaveBeenCalledWith("Details copied");
  });

  it("omits an empty or duplicate detail and accepts a custom next step", () => {
    const onClick = vi.fn();
    showError({
      title: "Proxy failed",
      error: new Error("Proxy failed"),
      action: { label: "Open settings", onClick },
    });
    const [, options] = toastMock.error.mock.calls[0];
    expect(options.description).toBeUndefined();
    expect(options.action).toEqual({ label: "Open settings", onClick });
    expect(errorReport("Proxy failed", undefined)).toBe("Proxy failed");
  });
});
