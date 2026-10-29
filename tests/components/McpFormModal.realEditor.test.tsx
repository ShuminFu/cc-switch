import React from "react";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
} from "@testing-library/react";
import { EditorView } from "codemirror";
import McpFormModal from "@/components/mcp/McpFormModal";

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

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

// t 必须在各次渲染间保持同一引用（与真实 react-i18next 一致），
// 否则 JsonEditor 的 linter 依赖每次变化会重建编辑器，掩盖陈旧 onChange 的问题
const stableT = vi.hoisted(
  () => (key: string, params?: Record<string, unknown>) =>
    params ? `${key}:${JSON.stringify(params)}` : key,
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableT }),
  // 提供 initReactI18next 以兼容 i18n 初始化路径
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@/config/mcpPresets", () => ({
  mcpPresets: [
    {
      id: "preset-stdio",
      server: { type: "stdio", command: "preset-cmd" },
    },
  ],
  getMcpPresetWithDescription: (preset: Record<string, unknown>) => ({
    ...preset,
    description: "Preset description",
    tags: ["preset"],
  }),
}));

vi.mock("@/hooks/useMcp", async () => {
  const actual =
    await vi.importActual<typeof import("@/hooks/useMcp")>("@/hooks/useMcp");
  return {
    ...actual,
    useUpsertMcpServer: () => ({
      mutateAsync: vi.fn().mockResolvedValue(undefined),
    }),
  };
});

// 不 mock JsonEditor：使用真实的 CodeMirror 编辑器，覆盖 onChange 闭包陈旧的回归（CCS-105）
const replaceEditorDoc = (text: string) => {
  const editorDom = document.querySelector(".cm-editor") as HTMLElement;
  const view = EditorView.findFromDOM(editorDom);
  if (!view) throw new Error("EditorView not found");
  act(() => {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    });
  });
};

describe("McpFormModal with real JsonEditor", () => {
  const renderForm = (
    props?: Partial<React.ComponentProps<typeof McpFormModal>>,
  ) =>
    render(
      <McpFormModal
        onSave={vi.fn().mockResolvedValue(undefined)}
        onClose={vi.fn()}
        existingIds={[]}
        defaultFormat="json"
        {...props}
      />,
    );

  it("JSON 编辑不会覆盖已输入的 ID 与名称", async () => {
    renderForm();

    const idInput = (await screen.findByPlaceholderText(
      "mcp.form.titlePlaceholder",
    )) as HTMLInputElement;
    const nameInput = screen.getByPlaceholderText(
      "mcp.form.namePlaceholder",
    ) as HTMLInputElement;

    fireEvent.change(idInput, { target: { value: "my-id" } });
    fireEvent.change(nameInput, { target: { value: "My Name" } });

    replaceEditorDoc('{"foo":{"type":"stdio","command":"run"}}');

    await waitFor(() => expect(idInput.value).toBe("my-id"));
    expect(nameInput.value).toBe("My Name");
  });

  it("JSON 编辑不会覆盖预设填充的 ID", async () => {
    renderForm();

    await screen.findByPlaceholderText("mcp.form.titlePlaceholder");
    fireEvent.click(screen.getByText("preset-stdio"));

    const idInput = screen.getByPlaceholderText(
      "mcp.form.titlePlaceholder",
    ) as HTMLInputElement;
    expect(idInput.value).toBe("preset-stdio");

    replaceEditorDoc('{"foo":{"type":"stdio","command":"run"}}');

    await waitFor(() => expect(idInput.value).toBe("preset-stdio"));
  });

  it("TOML 编辑不会覆盖已输入的 ID", async () => {
    renderForm({ defaultFormat: "toml" });

    const idInput = (await screen.findByPlaceholderText(
      "mcp.form.titlePlaceholder",
    )) as HTMLInputElement;
    fireEvent.change(idInput, { target: { value: "my-id" } });

    replaceEditorDoc('[mcp_servers.foo]\ntype = "stdio"\ncommand = "run"\n');

    await waitFor(() => expect(idInput.value).toBe("my-id"));
  });

  it("ID 为空时仍从 JSON 中提取 ID 与名称", async () => {
    renderForm();

    const idInput = (await screen.findByPlaceholderText(
      "mcp.form.titlePlaceholder",
    )) as HTMLInputElement;
    const nameInput = screen.getByPlaceholderText(
      "mcp.form.namePlaceholder",
    ) as HTMLInputElement;

    replaceEditorDoc('{"foo":{"type":"stdio","command":"run"}}');

    await waitFor(() => expect(idInput.value).toBe("foo"));
    expect(nameInput.value).toBe("foo");
  });
});
