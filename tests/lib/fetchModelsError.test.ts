import { describe, expect, it, vi, beforeEach } from "vitest";
import { classifyFetchModelsError } from "@/lib/errors/fetchModelsError";
import { BackendError } from "@/lib/errors/appError";

const toastErrorMock = vi.fn();
vi.mock("sonner", () => ({
  toast: { error: (...args: unknown[]) => toastErrorMock(...args) },
}));

// FE-ARCH-11 / FE-ARCH-29: one classifier for model-fetch failures, prefers
// structured error codes, toasts live in the UI layer.
describe("classifyFetchModelsError", () => {
  it("reads the status of a structured HttpStatus error", () => {
    const err = new BackendError({
      code: "errors.httpStatus",
      message: "HTTP 401: denied",
      params: { status: "401", body: "denied" },
    });
    expect(classifyFetchModelsError(err)).toBe("authFailed");
    expect(
      classifyFetchModelsError({
        code: "errors.httpStatus",
        message: "x",
        params: { status: "404" },
      }),
    ).toBe("endpointNotFound");
  });

  it("falls back to the legacy string messages", () => {
    expect(classifyFetchModelsError("HTTP 403 Forbidden")).toBe("authFailed");
    expect(classifyFetchModelsError("HTTP 405")).toBe("endpointNotFound");
    expect(classifyFetchModelsError("All candidates failed: ...")).toBe(
      "endpointNotFound",
    );
    expect(classifyFetchModelsError("request timed out")).toBe("timeout");
    expect(classifyFetchModelsError("Failed to parse body")).toBe(
      "notSupported",
    );
    expect(classifyFetchModelsError("boom")).toBe("unknown");
    // "HTTP 4010" is not a 401.
    expect(classifyFetchModelsError("HTTP 4010")).toBe("unknown");
  });
});

describe("showFetchModelsError", () => {
  beforeEach(() => toastErrorMock.mockReset());

  it("toasts exactly once with the mapped key", async () => {
    const { showFetchModelsError } = await import(
      "@/components/providers/forms/shared/showFetchModelsError"
    );
    const t = ((key: string) => key) as never;
    showFetchModelsError("HTTP 401", t);
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock).toHaveBeenCalledWith(
      "providerForm.fetchModelsAuthFailed",
    );
  });
});
