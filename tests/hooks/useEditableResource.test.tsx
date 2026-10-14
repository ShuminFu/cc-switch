import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useEditableResource } from "@/hooks/useEditableResource";
import { useCloseGuard } from "@/hooks/useCloseGuard";

type Doc = { text: string };

const setup = (initial: Doc | undefined) =>
  renderHook(
    ({ data }: { data: Doc | undefined }) =>
      useEditableResource<Doc, string>({
        data,
        toDraft: (d) => d.text,
        initialDraft: "",
      }),
    { initialProps: { data: initial } },
  );

describe("useEditableResource", () => {
  it("seeds synchronously when data is available on mount", () => {
    const { result } = setup({ text: "a" });
    expect(result.current.draft).toBe("a");
    expect(result.current.isSeeded).toBe(true);
    expect(result.current.isDirty).toBe(false);
  });

  it("uses initialDraft until data arrives, then seeds", () => {
    const { result, rerender } = setup(undefined);
    expect(result.current.draft).toBe("");
    expect(result.current.isSeeded).toBe(false);

    rerender({ data: { text: "loaded" } });
    expect(result.current.draft).toBe("loaded");
    expect(result.current.isSeeded).toBe(true);
  });

  it("silently re-seeds when data changes and there are no edits", () => {
    const { result, rerender } = setup({ text: "a" });
    rerender({ data: { text: "b" } });
    expect(result.current.draft).toBe("b");
    expect(result.current.externalChange).toBe(false);
  });

  it("derives dirtiness from content, so undoing an edit is clean again", () => {
    const { result } = setup({ text: "a" });
    act(() => result.current.setDraft("ab"));
    expect(result.current.isDirty).toBe(true);
    act(() => result.current.setDraft("a"));
    expect(result.current.isDirty).toBe(false);
  });

  it("keeps edits and flags an external change when data changes while dirty", () => {
    const { result, rerender } = setup({ text: "a" });
    act(() => result.current.setDraft("mine"));

    rerender({ data: { text: "theirs" } });
    expect(result.current.draft).toBe("mine");
    expect(result.current.externalChange).toBe(true);
  });

  it("ignores a refetch with unchanged content while dirty", () => {
    const { result, rerender } = setup({ text: "a" });
    act(() => result.current.setDraft("mine"));
    rerender({ data: { text: "a" } });
    expect(result.current.externalChange).toBe(false);
    expect(result.current.draft).toBe("mine");
  });

  it("reload replaces the draft with the external version", () => {
    const { result, rerender } = setup({ text: "a" });
    act(() => result.current.setDraft("mine"));
    rerender({ data: { text: "theirs" } });

    act(() => result.current.reload());
    expect(result.current.draft).toBe("theirs");
    expect(result.current.isDirty).toBe(false);
    expect(result.current.externalChange).toBe(false);
    expect(result.current.baselineData).toEqual({ text: "theirs" });
  });

  it("keepMine keeps the draft dirty against the new baseline", () => {
    const { result, rerender } = setup({ text: "a" });
    act(() => result.current.setDraft("mine"));
    const theirs = { text: "theirs" };
    rerender({ data: theirs });

    act(() => result.current.keepMine());
    expect(result.current.draft).toBe("mine");
    expect(result.current.isDirty).toBe(true);
    expect(result.current.externalChange).toBe(false);
    expect(result.current.baselineData).toBe(theirs);
  });

  it("markSaved clears dirtiness and lets the post-save refetch re-seed", () => {
    const { result, rerender } = setup({ text: "a" });
    act(() => result.current.setDraft("mine"));
    act(() => result.current.markSaved("mine"));
    expect(result.current.isDirty).toBe(false);

    rerender({ data: { text: "mine (normalized)" } });
    expect(result.current.draft).toBe("mine (normalized)");
    expect(result.current.externalChange).toBe(false);
  });

  it("edits made during a save stay dirty", () => {
    const { result } = setup({ text: "a" });
    act(() => result.current.setDraft("v1"));
    const snapshot = result.current.draft;
    act(() => result.current.setDraft("v2"));
    act(() => result.current.markSaved(snapshot));
    expect(result.current.isDirty).toBe(true);
  });
});

describe("useCloseGuard", () => {
  it("closes immediately when clean", () => {
    const onClose = vi.fn();
    const { result } = renderHook(() => useCloseGuard(false, onClose));
    act(() => result.current.requestClose());
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(result.current.confirmOpen).toBe(false);
  });

  it("asks first when dirty, and closes only after confirming", () => {
    const onClose = vi.fn();
    const onDiscard = vi.fn();
    const { result } = renderHook(() =>
      useCloseGuard(true, onClose, onDiscard),
    );

    act(() => result.current.requestClose());
    expect(result.current.confirmOpen).toBe(true);
    expect(onClose).not.toHaveBeenCalled();

    act(() => result.current.cancelDiscard());
    expect(result.current.confirmOpen).toBe(false);
    expect(onClose).not.toHaveBeenCalled();

    act(() => result.current.requestClose());
    act(() => result.current.confirmDiscard());
    expect(onDiscard).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
