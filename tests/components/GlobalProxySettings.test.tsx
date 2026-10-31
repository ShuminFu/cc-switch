import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";

const savedUrlState = vi.hoisted(() => ({ value: "http://127.0.0.1:7890" }));

const mutateAsyncMock = vi.fn();
const testMutateAsyncMock = vi.fn();
const scanMutateAsyncMock = vi.fn();

vi.mock("@/hooks/useGlobalProxy", () => ({
  useGlobalProxyUrl: () => ({ data: savedUrlState.value, isLoading: false }),
  useSetGlobalProxyUrl: () => ({
    mutateAsync: mutateAsyncMock,
    isPending: false,
  }),
  useTestProxy: () => ({
    mutateAsync: testMutateAsyncMock,
    isPending: false,
  }),
  useScanProxies: () => ({
    mutateAsync: scanMutateAsyncMock,
    isPending: false,
  }),
}));

describe("GlobalProxySettings", () => {
  beforeEach(() => {
    mutateAsyncMock.mockReset();
    testMutateAsyncMock.mockReset();
    scanMutateAsyncMock.mockReset();
    savedUrlState.value = "http://127.0.0.1:7890";
  });

  it("renders proxy URL input with saved value", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );
    // The saved string round-trips unchanged (no added trailing slash).
    await waitFor(() => expect(urlInput).toHaveValue("http://127.0.0.1:7890"));
  });

  it("saves proxy URL when save button is clicked", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );

    fireEvent.change(urlInput, { target: { value: "http://localhost:8080" } });

    const saveButton = screen.getByRole("button", { name: "Save" });
    fireEvent.click(saveButton);

    await waitFor(() => expect(mutateAsyncMock).toHaveBeenCalled());
    // 没有用户名时，URL 不经过 URL 对象解析，所以没有尾部斜杠
    expect(mutateAsyncMock).toHaveBeenCalledWith("http://localhost:8080");
  });

  it("clears proxy URL when clear button is clicked", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );

    // Wait for initial value to load
    await waitFor(() => expect(urlInput).toHaveValue("http://127.0.0.1:7890"));

    // Click clear button
    const clearButton = screen.getByTitle("Clear");
    fireEvent.click(clearButton);

    expect(urlInput).toHaveValue("");
  });

  it("splits embedded credentials and merges them back without rewriting the URL (FE-UI-43)", async () => {
    savedUrlState.value = "socks5://us%40r:p%3Ass@10.0.0.1:1080";
    render(<GlobalProxySettings />);

    const urlInput = screen.getByLabelText("Proxy URL");
    await waitFor(() => expect(urlInput).toHaveValue("socks5://10.0.0.1:1080"));
    expect(screen.getByLabelText("Username (optional)")).toHaveValue("us@r");
    expect(screen.getByLabelText("Password (optional)")).toHaveValue("p:ss");

    fireEvent.change(urlInput, { target: { value: "socks5://10.0.0.2:1080" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(mutateAsyncMock).toHaveBeenCalledWith(
        "socks5://us%40r:p%3Ass@10.0.0.2:1080",
      ),
    );
  });

  it("labels the password toggle and keeps it focusable", () => {
    render(<GlobalProxySettings />);
    const toggle = screen.getByRole("button", {
      name: "Show password",
    });
    expect(toggle).not.toHaveAttribute("tabindex", "-1");
    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });

  it("does not leak an unhandled rejection when saving fails", async () => {
    mutateAsyncMock.mockRejectedValue(new Error("boom"));
    render(<GlobalProxySettings />);
    fireEvent.change(screen.getByLabelText("Proxy URL"), {
      target: { value: "http://localhost:1" },
    });
    const save = screen.getByRole("button", { name: "Save" });
    fireEvent.click(save);
    await waitFor(() => expect(mutateAsyncMock).toHaveBeenCalled());
    // Still dirty, so the user can retry.
    expect(save).not.toBeDisabled();
  });
});
