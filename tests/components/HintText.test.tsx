import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  HintText,
  stripHintEmoji,
} from "@/components/providers/forms/shared/HintText";

describe("HintText (FE-PROV-15)", () => {
  it("moves the lightbulb out of the copy", () => {
    expect(stripHintEmoji("💡 Fill in the key")).toBe("Fill in the key");
    expect(stripHintEmoji("No emoji")).toBe("No emoji");
    const { container } = render(<HintText>💡 Fill in the key</HintText>);
    expect(container.textContent).toBe("Fill in the key");
    expect(container.querySelector("svg")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });
});
