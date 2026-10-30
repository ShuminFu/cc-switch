import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import type { ReactNode } from "react";
import { describe, expect, it } from "vitest";
import { server } from "../msw/server";
import { ProjectBindingsSection } from "@/components/projects/ProjectBindingsSection";
import type { Profile } from "@/lib/api/profiles";

const TAURI = "http://tauri.local";

const profile = {
  id: "p1",
  name: "Work",
  payload: {},
} as unknown as Profile;

const wrap = (ui: ReactNode) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>{ui}</QueryClientProvider>,
  );
};

describe("ProjectBindingsSection", () => {
  it("shows the empty state from the default handler", async () => {
    wrap(<ProjectBindingsSection profiles={[profile]} />);
    expect(
      await screen.findByText(/No folders bound yet|projects\.noBindings/),
    ).toBeInTheDocument();
  });

  it("lists bindings and applies one", async () => {
    let applied: string | undefined;
    server.use(
      http.post(`${TAURI}/list_project_bindings`, () =>
        HttpResponse.json({
          bindings: [
            {
              path: "/work/api",
              profileId: "p1",
              scopes: [],
              projectLevel: true,
              managed: [],
              createdAt: 0,
              updatedAt: 0,
            },
          ],
          projectLevelScopes: ["claude", "gemini"],
        }),
      ),
      http.post(`${TAURI}/apply_project_binding`, async ({ request }) => {
        const body = (await request.json()) as { path?: string };
        applied = body.path;
        return HttpResponse.json(null);
      }),
    );
    wrap(<ProjectBindingsSection profiles={[profile]} />);
    expect(await screen.findByText("/work/api")).toBeInTheDocument();
    expect(screen.getByText(/Work/)).toBeInTheDocument();

    const applyButtons = screen.getAllByRole("button", {
      name: /Apply now|projects\.apply/,
    });
    fireEvent.click(applyButtons[0]);
    await waitFor(() => expect(applied).toBe("/work/api"));
  });
});
