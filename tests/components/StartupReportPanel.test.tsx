import { render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { StartupReportPanel } from "@/components/settings/StartupReportPanel";
import type { StartupReport } from "@/shared/bindings";
import { command } from "../msw/handlers";
import { server } from "../msw/server";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key} ${JSON.stringify(opts)}` : key,
  }),
}));

const renderPanel = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <StartupReportPanel />
    </QueryClientProvider>,
  );
};

describe("StartupReportPanel", () => {
  it("lists every step and shows the error code of a failed step", async () => {
    const report: StartupReport = {
      steps: [
        {
          id: "database_init",
          status: "ok",
          code: null,
          message: null,
          durationMs: 3,
        },
        {
          id: "live_config_import",
          status: "failed",
          code: "JSON",
          message: "claude: invalid JSON",
          durationMs: 1,
        },
        {
          id: "mcp_import",
          status: "skipped",
          code: null,
          message: "MCP table not empty",
          durationMs: 0,
        },
      ],
    };
    server.use(command("get_startup_report", () => report));

    renderPanel();

    const failed = await screen.findByTestId("startup-step-live_config_import");
    expect(within(failed).getByText("JSON")).toBeInTheDocument();
    expect(
      within(failed).getByText("claude: invalid JSON"),
    ).toBeInTheDocument();
    expect(
      within(failed).getByText("settings.advanced.startupReport.status.failed"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        'settings.advanced.startupReport.failedSummary {"count":1}',
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("startup-step-database_init"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("startup-step-mcp_import")).toBeInTheDocument();
  });

  it("reports a clean startup when no step failed", async () => {
    server.use(
      command("get_startup_report", () => ({
        steps: [
          {
            id: "database_init",
            status: "ok",
            code: null,
            message: null,
            durationMs: 2,
          },
        ],
      })),
    );

    renderPanel();

    expect(
      await screen.findByText("settings.advanced.startupReport.allOk"),
    ).toBeInTheDocument();
  });
});
