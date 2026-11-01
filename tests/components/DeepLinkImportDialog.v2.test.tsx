import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { PromptConfirmation } from "@/components/deeplink/PromptConfirmation";
import type { DeepLinkImportRequestV2 } from "@/components/deeplink/api";
import { server } from "../msw/server";
import { emitTauriEvent } from "../msw/tauriMocks";

const TAURI = "http://tauri.local";

const b64 = (text: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(text)));

function renderDialog() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <DeepLinkImportDialog />
    </QueryClientProvider>,
  );
}

async function emit(request: DeepLinkImportRequestV2) {
  await act(async () => {
    emitTauriEvent("deeplink-import", request);
  });
}

async function clickImport() {
  const button = screen.getByRole("button", {
    name: /^import$|deeplink.import$/i,
  });
  await waitFor(() => expect(button).not.toBeDisabled(), { timeout: 3000 });
  fireEvent.click(button);
}

const skill = (repo: string, nonce: string): DeepLinkImportRequestV2 => ({
  version: "v1",
  resource: "skill",
  repo,
  nonce,
});

const providerItem = {
  version: "v2",
  resource: "provider" as const,
  app: "claude" as const,
  name: "Example Relay",
  endpoint: "https://api.example.com",
  apiKey: "sk-example-placeholder",
};
const mcpItem = {
  version: "v2",
  resource: "mcp" as const,
  apps: "claude",
  config: b64(
    JSON.stringify({ mcpServers: { docs: { command: "npx", args: ["x"] } } }),
  ),
};
const promptItem = {
  version: "v2",
  resource: "prompt" as const,
  app: "claude" as const,
  name: "Guidelines",
  content: b64("# Be concise"),
};

const signedBundle: DeepLinkImportRequestV2 = {
  ...providerItem,
  nonce: "bundle-nonce",
  bundle: {
    version: 2,
    items: [providerItem, mcpItem, promptItem],
    signature: {
      status: "verified",
      publisher: "Example Vendor",
      keyId: "892C52781F9B9986",
      trust: "trusted",
    },
  },
};

describe("DeepLinkImportDialog v2", () => {
  it("imports a signed bundle with per-item choices in one dialog", async () => {
    const calls: unknown[] = [];
    server.use(
      http.post(`${TAURI}/import_deeplink_bundle`, async ({ request }) => {
        const body = await request.json();
        calls.push(body);
        return HttpResponse.json([
          { index: 0, resource: "provider", ok: true },
          { index: 2, resource: "prompt", ok: true },
        ]);
      }),
    );
    renderDialog();
    await emit(signedBundle);

    const badge = await screen.findByTestId("deeplink-signed-badge");
    expect(badge.textContent).toContain("Example Vendor");
    expect(badge.textContent).toContain("892C52781F9B9986");
    for (const index of [0, 1, 2]) {
      expect(
        screen.getByTestId(`deeplink-bundle-item-${index}`),
      ).toBeInTheDocument();
    }

    // Reject the MCP item.
    const mcp = screen.getByTestId("deeplink-bundle-item-1");
    fireEvent.click(within(mcp).getAllByRole("checkbox")[0]);

    await clickImport();
    await waitFor(() => expect(calls).toHaveLength(1));
    const body = calls[0] as {
      nonce: string;
      choices: Array<{ index: number; accept: boolean; enabled?: boolean }>;
    };
    expect(body.nonce).toBe("bundle-nonce");
    expect(body.choices.find((c) => c.index === 1)?.accept).toBe(false);
    expect(body.choices.filter((c) => c.accept).map((c) => c.index)).toEqual([
      0, 2,
    ]);
    expect(body.choices.every((c) => c.enabled !== true)).toBe(true);
  });

  it("marks v1 links as unsigned", async () => {
    renderDialog();
    await emit(skill("owner/repo", "n1"));
    expect(
      await screen.findByTestId("deeplink-unsigned-badge"),
    ).toBeInTheDocument();
  });

  it("queues a second link instead of replacing the one under review", async () => {
    const discarded: string[] = [];
    server.use(
      http.post(`${TAURI}/discard_pending_deeplink`, async ({ request }) => {
        discarded.push(((await request.json()) as { nonce: string }).nonce);
        return HttpResponse.json(true);
      }),
    );
    renderDialog();
    await emit(skill("first/repo", "n-first"));
    expect(await screen.findByText("first/repo")).toBeInTheDocument();

    await emit(skill("second/repo", "n-second"));
    expect(screen.getByText("first/repo")).toBeInTheDocument();
    expect(screen.queryByText("second/repo")).not.toBeInTheDocument();
    expect(screen.getByTestId("deeplink-queue-notice")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    expect(await screen.findByText("second/repo")).toBeInTheDocument();
    expect(discarded).toEqual(["n-first"]);
  });

  it("sends the pending-import nonce with single imports", async () => {
    const received: DeepLinkImportRequestV2[] = [];
    server.use(
      http.post(
        `${TAURI}/import_from_deeplink_unified`,
        async ({ request }) => {
          const body = (await request.json()) as {
            request: DeepLinkImportRequestV2;
          };
          received.push(body.request);
          return HttpResponse.json({ type: "skill", key: "owner/repo" });
        },
      ),
    );
    renderDialog();
    await emit(skill("owner/repo", "n-skill"));
    await screen.findByText("owner/repo");
    await clickImport();
    await waitFor(() => expect(received).toHaveLength(1));
    expect(received[0].nonce).toBe("n-skill");
  });
});

describe("PromptConfirmation", () => {
  it("renders a 2 KB prompt in full with a line count", () => {
    const tail = "TAIL-MARKER-after-the-old-500-char-cutoff";
    const content = `${"line of text\n".repeat(160)}${tail}`;
    expect(content.length).toBeGreaterThan(2000);
    render(
      <PromptConfirmation
        request={{
          version: "v1",
          resource: "prompt",
          app: "claude",
          name: "Big",
          content: b64(content),
        }}
        enableNow={false}
        onEnableNowChange={() => {}}
      />,
    );
    const pre = screen.getByTestId("deeplink-prompt-content");
    expect(pre.textContent).toBe(content);
    expect(pre.textContent).toContain(tail);
    expect(screen.getByRole("checkbox")).not.toBeChecked();
  });
});
