import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CopyEnvProfileMenu } from "@/components/env/CopyEnvProfileMenu";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

async function openMenu() {
  const trigger = screen.getByRole("button", { name: "Copy as shell profile" });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.keyDown(trigger, { key: "Enter" });
  return await screen.findByRole("menu");
}

describe("CopyEnvProfileMenu (SVC-35)", () => {
  beforeEach(() => {
    toastMock.success.mockReset();
    toastMock.error.mockReset();
  });

  it("renders the profile for the chosen shell and copies it", async () => {
    const api = mockInvoke({
      render_provider_env_profile: "set -gx ANTHROPIC_BASE_URL 'https://x'\n",
      copy_text_to_clipboard: null,
    });
    render(<CopyEnvProfileMenu appId="claude" providerId="p1" />);
    await openMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: "fish" }));

    await waitFor(() =>
      expect(api.callsTo("copy_text_to_clipboard")).toHaveLength(1),
    );
    expect(api.callsTo("render_provider_env_profile")[0].args).toEqual({
      app: "claude",
      providerId: "p1",
      shell: "fish",
    });
    expect(api.callsTo("copy_text_to_clipboard")[0].args).toEqual({
      text: "set -gx ANTHROPIC_BASE_URL 'https://x'\n",
    });
    expect(toastMock.success).toHaveBeenCalled();
  });

  it("reports a backend error instead of copying", async () => {
    const api = mockInvoke({
      render_provider_env_profile: () => {
        throw new Error(
          "This provider has no environment variables to export.",
        );
      },
      copy_text_to_clipboard: null,
    });
    render(<CopyEnvProfileMenu appId="codex" providerId="p2" />);
    await openMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: ".env" }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
    expect(api.callsTo("copy_text_to_clipboard")).toHaveLength(0);
  });

  it("is hidden for apps without an environment-variable form", () => {
    const { container } = render(
      <CopyEnvProfileMenu appId="opencode" providerId="p3" />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
