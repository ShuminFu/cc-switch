import { useState } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";

function Nested({ onOuterClose }: { onOuterClose: () => void }) {
  const [outerOpen, setOuterOpen] = useState(true);
  const [innerOpen, setInnerOpen] = useState(false);
  return (
    <FullScreenPanel
      isOpen={outerOpen}
      title="outer"
      onClose={() => {
        onOuterClose();
        setOuterOpen(false);
      }}
    >
      <button type="button" onClick={() => setInnerOpen(true)}>
        open-inner
      </button>
      <FullScreenPanel
        isOpen={innerOpen}
        title="inner"
        onClose={() => setInnerOpen(false)}
      >
        <button type="button">inner-button</button>
      </FullScreenPanel>
    </FullScreenPanel>
  );
}

describe("FullScreenPanel nested Escape handling (FE-PROV-41)", () => {
  it("closes only the innermost panel on Escape", async () => {
    const onOuterClose = vi.fn();
    render(<Nested onOuterClose={onOuterClose} />);

    fireEvent.click(screen.getByText("open-inner"));
    const innerButton = await screen.findByText("inner-button");
    innerButton.focus();

    fireEvent.keyDown(innerButton, { key: "Escape" });

    await waitFor(() =>
      expect(screen.queryByText("inner-button")).not.toBeInTheDocument(),
    );
    expect(onOuterClose).not.toHaveBeenCalled();
    expect(screen.getByText("open-inner")).toBeInTheDocument();

    fireEvent.keyDown(screen.getByText("open-inner"), { key: "Escape" });
    expect(onOuterClose).toHaveBeenCalledTimes(1);
  });
});
