import { describe, expect, it, vi } from "vitest";
import { redactSecrets } from "@/utils/redact";

const copyTextMock = vi.fn(async (_text: string) => undefined);
vi.mock("@/lib/clipboard", () => ({
  copyText: (text: string) => copyTextMock(text),
}));
vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "light" }),
}));

// FE-PROV-23: secrets never reach a toast or the clipboard in clear text.
describe("redactSecrets", () => {
  it("masks well-known key shapes and keeps the last 4 characters", () => {
    expect(redactSecrets("bad key sk-ant-abcdefghijklmnop1234")).toBe(
      "bad key sk-ant-…1234",
    );
    expect(
      redactSecrets("Authorization: Bearer abcdefghijklmnopqrstuvwxyz"),
    ).toBe("Authorization: Bearer …wxyz");
  });

  it("masks key = value assignments in JSON, TOML and env text", () => {
    expect(
      redactSecrets('{"ANTHROPIC_AUTH_TOKEN": "my-relay-token-5678"}'),
    ).toBe('{"ANTHROPIC_AUTH_TOKEN": "…5678"}');
    expect(
      redactSecrets("experimental_bearer_token = 'abcdefgh9999'"),
    ).toContain("…9999");
    expect(
      redactSecrets("GEMINI_API_KEY=AIzaSyA1234567890abcdefghij"),
    ).not.toContain("1234567890");
  });

  it("leaves ordinary text alone", () => {
    const text = "HTTP 401: invalid credentials for https://api.example.com";
    expect(redactSecrets(text)).toBe(text);
  });
});

// FE-ARCH-36: every error toast stays >= 6 s, can be closed and copied.
describe("errorToastOptions", () => {
  it("adds duration, close button and a redacted copy action", async () => {
    const { errorToastOptions, ERROR_TOAST_DURATION_MS } = await import(
      "@/components/ui/sonner"
    );
    const options = errorToastOptions("Save failed", {
      description: "token=sk-abcdefghijklmnop0000",
    });
    expect(options.duration).toBe(ERROR_TOAST_DURATION_MS);
    expect(options.closeButton).toBe(true);
    expect(options.description).toBe("token=sk-…0000");
    const action = options.action as unknown as { onClick: () => void };
    action.onClick();
    expect(copyTextMock).toHaveBeenCalledWith("Save failed\ntoken=sk-…0000");
  });

  it("keeps a caller-provided action and duration", async () => {
    const { errorToastOptions } = await import("@/components/ui/sonner");
    const own = { label: "Retry", onClick: () => undefined };
    const options = errorToastOptions("x", { action: own, duration: 1000 });
    expect(options.action).toBe(own);
    expect(options.duration).toBe(1000);
  });
});
