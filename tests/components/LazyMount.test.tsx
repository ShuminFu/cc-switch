import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { LazyMount, lazyNamed } from "@/components/common/LazyMount";

function Probe({ label }: { label: string }) {
  return <div data-testid="probe">{label}</div>;
}

describe("LazyMount", () => {
  it("does not load the lazy chunk until first opened, then keeps it mounted", async () => {
    const loader = vi.fn(() => Promise.resolve({ Probe }));
    const LazyProbe = lazyNamed(loader, "Probe");

    const { rerender } = render(
      <LazyMount when={false}>
        <LazyProbe label="hello" />
      </LazyMount>,
    );
    expect(loader).not.toHaveBeenCalled();
    expect(screen.queryByTestId("probe")).toBeNull();

    rerender(
      <LazyMount when={true}>
        <LazyProbe label="hello" />
      </LazyMount>,
    );
    expect((await screen.findByTestId("probe")).textContent).toBe("hello");
    expect(loader).toHaveBeenCalledTimes(1);

    // 关闭后仍保持挂载，交由子组件的 open 控制退出动画
    rerender(
      <LazyMount when={false}>
        <LazyProbe label="closed" />
      </LazyMount>,
    );
    expect(screen.getByTestId("probe").textContent).toBe("closed");
  });
});
