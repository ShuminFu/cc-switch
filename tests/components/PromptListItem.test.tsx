import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import PromptListItem from "@/components/prompts/PromptListItem";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

const prompt = {
  id: "p1",
  name: "Review",
  content: "# review",
  description: "strict",
  enabled: false,
};

describe("PromptListItem", () => {
  it("shows the share action only when a handler is provided", () => {
    const onShare = vi.fn();
    const { rerender } = render(
      <PromptListItem
        id="p1"
        prompt={prompt}
        onToggle={() => {}}
        onEdit={() => {}}
        onDelete={() => {}}
      />,
    );
    expect(screen.queryByRole("button", { name: "复制导入链接" })).toBeNull();

    rerender(
      <PromptListItem
        id="p1"
        prompt={prompt}
        onToggle={() => {}}
        onEdit={() => {}}
        onShare={onShare}
        onDelete={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "复制导入链接" }));
    expect(onShare).toHaveBeenCalledWith("p1");
  });
});
