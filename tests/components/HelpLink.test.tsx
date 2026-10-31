import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, describe, expect, it } from "vitest";
import { HelpLink } from "@/components/common/HelpLink";
import { MANUAL_BASE_URL } from "@/lib/helpLinks";
import { mockInvoke } from "../msw/invokeRecorder";

describe("HelpLink (DOCS-16)", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("opens the English manual page for the topic", async () => {
    const api = mockInvoke({ open_external: null });
    render(<HelpLink topic="proxy.failover" />);

    fireEvent.click(screen.getByRole("button", { name: "Open the manual" }));

    await waitFor(() =>
      expect(api.callsTo("open_external")).toEqual([
        {
          command: "open_external",
          args: { url: `${MANUAL_BASE_URL}/en/4-proxy/4.3-failover.md` },
        },
      ]),
    );
  });

  it("follows the UI language (zh-TW reads the zh manual)", async () => {
    await i18n.changeLanguage("zh-TW");
    const api = mockInvoke({ open_external: null });
    render(<HelpLink topic="mcp" />);

    fireEvent.click(screen.getByRole("button"));

    await waitFor(() =>
      expect(api.callsTo("open_external")[0]?.args).toEqual({
        url: `${MANUAL_BASE_URL}/zh/3-extensions/3.1-mcp.md`,
      }),
    );
  });
});
