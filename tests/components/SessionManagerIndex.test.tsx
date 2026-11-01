import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";
import { filterSessionsByQuery } from "@/components/sessions/sessionFilter";
import type { SessionMessage, SessionMeta } from "@/types";
import { server } from "../msw/server";
import { setSessionFixtures } from "../msw/state";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("@/components/sessions/SessionToc", () => ({
  SessionTocSidebar: () => null,
  SessionTocDialog: () => null,
}));

const TAURI = "http://tauri.local";

const sessions: SessionMeta[] = [
  {
    providerId: "codex",
    sessionId: "s-alpha",
    title: "Alpha Session",
    projectDir: "/mock/alpha",
    lastActiveAt: 20,
    sourcePath: "/mock/alpha.jsonl",
  },
  {
    providerId: "codex",
    sessionId: "s-beta",
    title: "Beta Session",
    projectDir: "/mock/beta",
    lastActiveAt: 10,
    sourcePath: "/mock/beta.jsonl",
  },
];

const messages: Record<string, SessionMessage[]> = {
  "codex:/mock/alpha.jsonl": [{ role: "user", content: "hello alpha" }],
  "codex:/mock/beta.jsonl": [
    { role: "user", content: "the flux capacitor is broken" },
  ],
};

const renderPage = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <SessionManagerPage appId="codex" />
    </QueryClientProvider>,
  );
  return client;
};

const openSearch = () => {
  const button = screen
    .getAllByRole("button")
    .find((b) => b.querySelector(".lucide-search"));
  if (!button) throw new Error("search button not found");
  fireEvent.click(button);
};

describe("SessionManagerPage with the session index", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    window.localStorage.clear();
    setSessionFixtures(sessions, messages);
  });

  it("searches transcript contents through the backend index", async () => {
    renderPage();
    await screen.findByRole("heading", { name: "Alpha Session" });

    openSearch();
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "flux capacitor" },
    });
    // Metadata search alone matches nothing.
    await waitFor(() =>
      expect(screen.queryByText("Beta Session")).not.toBeInTheDocument(),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Search message contents" }),
    );
    await waitFor(
      () =>
        expect(screen.getAllByText("Beta Session").length).toBeGreaterThan(0),
      { timeout: 3000 },
    );
    expect(screen.queryByText("Alpha Session")).not.toBeInTheDocument();
  });

  it("shows a banner naming providers whose sessions could not be read", async () => {
    server.use(
      http.post(`${TAURI}/scan_sessions_report`, () =>
        HttpResponse.json({
          sessions,
          errors: [{ providerId: "gemini", message: "Permission denied" }],
          generation: 1,
          changed: true,
          parsedFiles: 2,
        }),
      ),
    );
    renderPage();
    const banner = await screen.findByText(/Permission denied/);
    expect(banner.textContent).toMatch(/gemini/i);
    expect(screen.getAllByText("Alpha Session").length).toBeGreaterThan(0);
  });

  it("shows a retry state when the transcript fails to load", async () => {
    server.use(
      http.post(`${TAURI}/get_session_messages`, () =>
        HttpResponse.json("Session source is outside provider roots", {
          status: 500,
        }),
      ),
    );
    renderPage();
    await screen.findByRole("heading", { name: "Alpha Session" });
    expect(
      await screen.findByText("Failed to load this session"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("reloads the open transcript when refresh is clicked", async () => {
    const loads: string[] = [];
    server.use(
      http.post(`${TAURI}/get_session_messages`, async ({ request }) => {
        const body = (await request.json()) as { sourcePath: string };
        loads.push(body.sourcePath);
        return HttpResponse.json([{ role: "user", content: "hello alpha" }]);
      }),
    );
    renderPage();
    await screen.findByRole("heading", { name: "Alpha Session" });
    await waitFor(() => expect(loads.length).toBe(1));

    const refresh = screen
      .getAllByRole("button")
      .find((b) => b.querySelector(".lucide-refresh-cw"));
    fireEvent.click(refresh!);
    await waitFor(() => expect(loads.length).toBe(2));
    expect(loads[1]).toBe("/mock/alpha.jsonl");
  });
});

describe("filterSessionsByQuery", () => {
  it("matches every term as a substring and sorts by recency", () => {
    const result = filterSessionsByQuery(sessions, "SESSION mock/b");
    expect(result.map((s) => s.sessionId)).toEqual(["s-beta"]);
    expect(filterSessionsByQuery(sessions, "").map((s) => s.sessionId)).toEqual(
      ["s-alpha", "s-beta"],
    );
  });
});
