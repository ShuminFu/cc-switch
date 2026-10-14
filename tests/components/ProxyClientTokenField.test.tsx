import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";

import { ProxyClientTokenField } from "@/components/proxy/ProxyClientTokenField";

const getTokenMock = vi.fn();
const regenerateMock = vi.fn();
const toastSuccessMock = vi.fn();

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getProxyClientToken: () => getTokenMock(),
    regenerateProxyClientToken: () => regenerateMock(),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => (
    <button {...props}>{children}</button>
  ),
}));

describe("ProxyClientTokenField", () => {
  beforeEach(() => {
    getTokenMock.mockReset().mockResolvedValue("ccsp-first");
    regenerateMock.mockReset().mockResolvedValue("ccsp-second");
    toastSuccessMock.mockReset();
  });

  it("masks the token until revealed", async () => {
    render(<ProxyClientTokenField />);
    const value = screen.getByTestId("proxy-client-token-value");
    await waitFor(() => expect(getTokenMock).toHaveBeenCalled());
    await waitFor(() => expect(value).not.toHaveTextContent("…"));
    expect(value).not.toHaveTextContent("ccsp-first");

    fireEvent.click(screen.getByText("proxy.settings.fields.clientToken.show"));
    expect(value).toHaveTextContent("ccsp-first");
  });

  it("regenerates the token", async () => {
    render(<ProxyClientTokenField />);
    const regenerate = screen.getByText(
      "proxy.settings.fields.clientToken.regenerate",
    );
    await waitFor(() => expect(regenerate).not.toBeDisabled());
    fireEvent.click(regenerate);
    await waitFor(() => expect(regenerateMock).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByText("proxy.settings.fields.clientToken.show"));
    await waitFor(() =>
      expect(screen.getByTestId("proxy-client-token-value")).toHaveTextContent(
        "ccsp-second",
      ),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "proxy.settings.fields.clientToken.regenerated",
      expect.anything(),
    );
  });
});
