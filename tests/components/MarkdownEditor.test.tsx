import { render, act } from "@testing-library/react";
import { EditorView } from "codemirror";
import MarkdownEditor from "@/components/MarkdownEditor";

// jsdom 未实现 Range 的布局 API，CodeMirror 测量时会调用
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () =>
    ({
      length: 0,
      item: () => null,
      [Symbol.iterator]: [][Symbol.iterator],
    }) as unknown as DOMRectList;
}
if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = () => new DOMRect();
}

// 通过 CodeMirror 的 DOM 找到 EditorView，模拟用户编辑文档
const replaceDoc = (container: HTMLElement, text: string) => {
  const editorDom = container.querySelector(".cm-editor") as HTMLElement;
  const view = EditorView.findFromDOM(editorDom);
  if (!view) throw new Error("EditorView not found");
  act(() => {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    });
  });
};

describe("MarkdownEditor", () => {
  it("文档变化时调用最新一次渲染传入的 onChange", () => {
    const onChangeA = vi.fn();
    const onChangeB = vi.fn();

    const { container, rerender } = render(
      <MarkdownEditor value="" onChange={onChangeA} />,
    );
    rerender(<MarkdownEditor value="" onChange={onChangeB} />);

    replaceDoc(container, "# hello");

    expect(onChangeB).toHaveBeenCalledWith("# hello");
    expect(onChangeA).not.toHaveBeenCalled();
  });

  it("只读模式下不调用 onChange", () => {
    const onChange = vi.fn();

    const { container } = render(
      <MarkdownEditor value="" onChange={onChange} readOnly />,
    );

    replaceDoc(container, "# hello");

    expect(onChange).not.toHaveBeenCalled();
  });
});
