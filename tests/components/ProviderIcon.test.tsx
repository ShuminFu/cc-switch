import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProviderIcon, svgIconRendering } from "@/components/ProviderIcon";

const decode = (src: string) =>
  decodeURIComponent(src.replace(/^data:image\/svg\+xml;charset=utf-8,/, ""));

describe("ProviderIcon without inline markup (FE-ARCH-21)", () => {
  it("renders bundled SVG icons without injecting their markup", () => {
    const { container } = render(<ProviderIcon icon="anthropic" name="A" />);
    expect(container.querySelector("svg")).toBeNull();
    expect(screen.getByRole("img", { name: "A" })).toBeInTheDocument();
  });

  it("uses a mask for currentColor-only icons", () => {
    const svg =
      '<svg fill="currentColor" viewBox="0 0 24 24"><path d="M0 0h24v24H0z"/></svg>';
    const out = svgIconRendering(svg);
    expect(out.kind).toBe("mask");
    expect(decode(out.src)).toBe(svg);
  });

  it("draws coloured icons as images, resolving currentColor parts", () => {
    const svg =
      '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M0"/><path fill="#F90" d="M1"/></svg>';
    const out = svgIconRendering(svg, "#123456");
    expect(out.kind).toBe("image");
    expect(decode(out.src)).not.toContain("currentColor");
    expect(decode(out.src)).toContain('fill="#123456"');

    const plain = '<svg viewBox="0 0 1 1"><path fill="#E4906E" d="M0"/></svg>';
    expect(svgIconRendering(plain)).toEqual({
      kind: "image",
      src: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(plain)}`,
    });
  });

  it("a script-bearing string stays inert data", () => {
    const evil = '<svg onload="alert(1)"><script>alert(2)</script></svg>';
    const out = svgIconRendering(evil);
    expect(out.src.startsWith("data:image/svg+xml")).toBe(true);
  });
});
