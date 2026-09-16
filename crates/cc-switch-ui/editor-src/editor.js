// CodeMirror 6 bundle for the Dioxus UI. Built by tools/build-editor.sh into
// assets/editor.js and exposed as `window.CCEditor`. Port of the setup in
// src/components/JsonEditor.tsx and MarkdownEditor.tsx.
import { EditorView, basicSetup } from "codemirror";
import { EditorState, Compartment } from "@codemirror/state";
import { placeholder as placeholderExt } from "@codemirror/view";
import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";
import { markdown } from "@codemirror/lang-markdown";
import { oneDark } from "@codemirror/theme-one-dark";
import { linter } from "@codemirror/lint";

const baseTheme = EditorView.baseTheme({
  ".cm-editor": { border: "1px solid hsl(var(--border))", borderRadius: "0.5rem", background: "transparent" },
  ".cm-editor.cm-focused": { outline: "none", borderColor: "hsl(var(--primary))" },
  ".cm-scroller": { background: "transparent" },
  ".cm-gutters": { background: "transparent", borderRight: "1px solid hsl(var(--border))", color: "hsl(var(--muted-foreground))" },
  ".cm-selectionBackground, .cm-content ::selection": { background: "hsl(var(--primary) / 0.18)" },
  ".cm-selectionMatch": { background: "hsl(var(--primary) / 0.12)" },
  ".cm-activeLine": { background: "hsl(var(--primary) / 0.08)" },
  ".cm-activeLineGutter": { background: "hsl(var(--primary) / 0.08)" },
});

const darkOverrides = EditorView.theme({
  ".cm-editor": { border: "1px solid hsl(var(--border))", background: "transparent" },
  "&": { backgroundColor: "transparent" },
  ".cm-gutters": { background: "transparent" },
});

function languageExt(language) {
  switch (language) {
    case "javascript": return javascript();
    case "markdown": return markdown();
    case "json": return json();
    default: return [];
  }
}

// JSON validation identical to JsonEditor.tsx: must parse and be a plain object.
function jsonLinter(messages) {
  return linter((view) => {
    const doc = view.state.doc.toString();
    if (!doc.trim()) return [];
    try {
      const parsed = JSON.parse(doc);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) return [];
      return [{ from: 0, to: doc.length, severity: "error", message: messages.mustBeObject }];
    } catch (e) {
      return [{ from: 0, to: doc.length, severity: "error", message: e instanceof SyntaxError ? e.message : messages.invalidJson }];
    }
  });
}

function mount(parent, opts, onChange) {
  const theme = new Compartment();
  const readOnly = new Compartment();
  const heightValue = opts.height ? (typeof opts.height === "number" ? `${opts.height}px` : opts.height) : undefined;
  const minHeightPx = Math.max(1, opts.rows ?? 12) * 18;
  const sizingTheme = EditorView.theme({
    "&": heightValue ? { height: heightValue } : { minHeight: `${minHeightPx}px` },
    ".cm-scroller": { overflow: "auto" },
    ".cm-content": {
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace",
      fontSize: "14px",
    },
  });
  const extensions = [
    basicSetup,
    languageExt(opts.language),
    placeholderExt(opts.placeholder || ""),
    baseTheme,
    sizingTheme,
    theme.of(opts.dark ? [oneDark, darkOverrides] : []),
    readOnly.of(EditorState.readOnly.of(!!opts.readOnly)),
    EditorView.updateListener.of((update) => {
      if (update.docChanged && onChange) onChange(update.state.doc.toString());
    }),
  ];
  if (opts.language === "json" && opts.validate !== false) {
    extensions.push(jsonLinter({ mustBeObject: opts.mustBeObjectMessage || "Must be a JSON object", invalidJson: opts.invalidJsonMessage || "Invalid JSON" }));
  }
  const view = new EditorView({ state: EditorState.create({ doc: opts.value || "", extensions }), parent });
  return {
    getValue: () => view.state.doc.toString(),
    setValue: (value) => {
      const current = view.state.doc.toString();
      if (value === current) return;
      view.dispatch({ changes: { from: 0, to: current.length, insert: value } });
    },
    setDark: (dark) => view.dispatch({ effects: theme.reconfigure(dark ? [oneDark, darkOverrides] : []) }),
    setReadOnly: (ro) => view.dispatch({ effects: readOnly.reconfigure(EditorState.readOnly.of(!!ro)) }),
    focus: () => view.focus(),
    destroy: () => view.destroy(),
  };
}

window.CCEditor = { mount };
