import type { ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import {
  isValidProviderKey,
  sanitizeProviderKeyInput,
} from "@/components/providers/forms/adapters/providerKey";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      data-testid="json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

vi.setConfig({ testTimeout: 30000 });

function renderWithClient(node: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>{node}</QueryClientProvider>,
  );
}

const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });

beforeEach(() => {
  vi.mocked(toast.error).mockClear();
});

describe("provider key helpers (FE-PROV-52)", () => {
  it("validates and sanitises keys in one place", () => {
    expect(isValidProviderKey("my-relay-2")).toBe(true);
    expect(isValidProviderKey("-bad")).toBe(false);
    expect(isValidProviderKey("a--b")).toBe(false);
    expect(sanitizeProviderKeyInput("My Relay_2!")).toBe("myrelay2");
  });
});

describe("additive provider key guard (FE-PROV-44)", () => {
  it("refuses a key that already exists in the live config", async () => {
    mockInvoke({ get_openclaw_live_provider_ids: ["taken"] });
    const onSubmit = vi.fn();
    renderWithClient(
      <ProviderForm
        appId="openclaw"
        submitLabel="add"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    const keyInput = (await screen.findByLabelText(
      /providerKey|Provider Key|供应商标识/i,
    )) as HTMLInputElement;
    await waitFor(() => expect(keyInput).not.toBeDisabled());
    fireEvent.change(keyInput, { target: { value: "taken" } });
    fireEvent.change(document.querySelector('input[name="name"]')!, {
      target: { value: "Relay" },
    });
    expect(keyInput).toHaveAttribute("aria-invalid", "true");

    fireEvent.submit(document.getElementById("provider-form")!);
    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("refuses to add while the live key lookup failed", async () => {
    mockInvoke({
      get_hermes_live_provider_ids: () => {
        throw new Error("config.yaml is not valid YAML");
      },
    });
    const onSubmit = vi.fn();
    renderWithClient(
      <ProviderForm
        appId="hermes"
        submitLabel="add"
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    const keyInput = document.getElementById("hermes-key") as HTMLInputElement;
    await waitFor(() => expect(keyInput).toBeDisabled());
    fireEvent.change(document.querySelector('input[name="name"]')!, {
      target: { value: "Relay" },
    });
    await settle();

    fireEvent.submit(document.getElementById("provider-form")!);
    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

describe("only the active adapter is mounted (FE-PROV-17 / FE-PROV-52)", () => {
  it("the Claude form does not query OpenClaw / Hermes / OpenCode providers", async () => {
    const { calls } = mockInvoke({
      get_providers: () => ({}),
      get_openclaw_live_provider_ids: [],
      get_hermes_live_provider_ids: [],
      get_opencode_live_provider_ids: [],
    });
    renderWithClient(
      <ProviderForm
        appId="claude"
        submitLabel="add"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    await screen.findByRole("button", { name: /PatewayAI/ });
    await settle();
    const queriedApps = calls
      .filter((call) => call.command === "get_providers")
      .map((call) => call.args.app);
    expect(queriedApps).not.toContain("openclaw");
    expect(queriedApps).not.toContain("hermes");
    expect(queriedApps).not.toContain("opencode");
    expect(
      calls.filter((call) => call.command.endsWith("_live_provider_ids")),
    ).toEqual([]);
  });
});

describe("common-config first-run notice (FE-PROV-27)", () => {
  const renderForm = (appId: "claude" | "hermes") =>
    renderWithClient(
      <ProviderForm
        appId={appId}
        submitLabel="add"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );

  it("is not shown by apps without common config", async () => {
    mockInvoke({ get_settings: { commonConfigConfirmed: false } });
    renderForm("hermes");
    await waitFor(() =>
      expect(document.getElementById("hermes-key")).not.toBeNull(),
    );
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("is still shown by apps that have common config", async () => {
    mockInvoke({ get_settings: { commonConfigConfirmed: false } });
    renderForm("claude");
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });
});
