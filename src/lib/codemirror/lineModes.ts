import { RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import type { Diagnostic } from "@codemirror/lint";
import TOML from "smol-toml";

/**
 * Lightweight line-based highlighting for TOML (`config.toml`) and dotenv
 * (`.env`) text, plus TOML diagnostics positioned at the parser's line and
 * column (FE-PROV-16). Both formats are line oriented, so a per-line
 * tokenizer is enough and needs no extra CodeMirror language package.
 */

export type LineMode = "toml" | "dotenv";
export type TokenKind = "comment" | "header" | "key" | "string" | "atom";

export interface LineToken {
  from: number;
  to: number;
  kind: TokenKind;
}

const TOML_KEY =
  /^(\s*)((?:[A-Za-z0-9_-]+|"[^"]*"|'[^']*')(?:\s*\.\s*(?:[A-Za-z0-9_-]+|"[^"]*"|'[^']*'))*)\s*=/;
const ATOM =
  /^(?:true|false|[+-]?(?:inf|nan)|[+-]?\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)\b/;
const DOTENV_KEY = /^(\s*(?:export\s+)?)([A-Za-z_][A-Za-z0-9_.-]*)\s*=/;

/** Scan a value region for strings, atoms and a trailing comment. */
function scanValue(line: string, start: number, out: LineToken[]) {
  let i = start;
  while (i < line.length) {
    const ch = line[i];
    if (ch === "#") {
      out.push({ from: i, to: line.length, kind: "comment" });
      return;
    }
    if (ch === '"' || ch === "'") {
      let j = i + 1;
      while (j < line.length && line[j] !== ch) {
        if (ch === '"' && line[j] === "\\") j += 1;
        j += 1;
      }
      const end = Math.min(j + 1, line.length);
      out.push({ from: i, to: end, kind: "string" });
      i = end;
      continue;
    }
    const prev = i === 0 ? " " : line[i - 1];
    if (/[\s=,[{]/.test(prev)) {
      const atom = ATOM.exec(line.slice(i));
      if (atom) {
        out.push({ from: i, to: i + atom[0].length, kind: "atom" });
        i += atom[0].length;
        continue;
      }
    }
    i += 1;
  }
}

/** Tokens of one line, as offsets into that line. */
export function tokenizeLine(mode: LineMode, line: string): LineToken[] {
  const out: LineToken[] = [];
  const trimmed = line.trimStart();
  const indent = line.length - trimmed.length;
  if (trimmed.startsWith("#")) {
    return [{ from: indent, to: line.length, kind: "comment" }];
  }
  if (mode === "dotenv") {
    const key = DOTENV_KEY.exec(line);
    if (!key) return out;
    const keyFrom = key[1].length;
    out.push({ from: keyFrom, to: keyFrom + key[2].length, kind: "key" });
    const valueFrom = key[0].length;
    const rest = line.slice(valueFrom);
    const valueStart = valueFrom + (rest.length - rest.trimStart().length);
    if (valueStart < line.length) {
      out.push({ from: valueStart, to: line.length, kind: "string" });
    }
    return out;
  }
  if (trimmed.startsWith("[")) {
    const close = line.indexOf("]", indent);
    const end =
      close === -1
        ? line.length
        : line[close + 1] === "]"
          ? close + 2
          : close + 1;
    out.push({ from: indent, to: end, kind: "header" });
    scanValue(line, end, out);
    return out;
  }
  const key = TOML_KEY.exec(line);
  if (key) {
    const keyFrom = key[1].length;
    out.push({ from: keyFrom, to: keyFrom + key[2].length, kind: "key" });
    scanValue(line, key[0].length, out);
    return out;
  }
  // Continuation lines of multi-line arrays / inline tables.
  scanValue(line, 0, out);
  return out;
}

const marks: Record<TokenKind, Decoration> = {
  comment: Decoration.mark({ class: "cm-lm-comment" }),
  header: Decoration.mark({ class: "cm-lm-header" }),
  key: Decoration.mark({ class: "cm-lm-key" }),
  string: Decoration.mark({ class: "cm-lm-string" }),
  atom: Decoration.mark({ class: "cm-lm-atom" }),
};

function buildDecorations(view: EditorView, mode: LineMode): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    let pos = from;
    while (pos <= to) {
      const line = view.state.doc.lineAt(pos);
      for (const token of tokenizeLine(mode, line.text)) {
        if (token.to > token.from) {
          builder.add(
            line.from + token.from,
            line.from + token.to,
            marks[token.kind],
          );
        }
      }
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

const lineModeTheme = EditorView.baseTheme({
  ".cm-lm-comment": { color: "#6a737d", fontStyle: "italic" },
  ".cm-lm-header": { color: "#6f42c1", fontWeight: "600" },
  ".cm-lm-key": { color: "#005cc5" },
  ".cm-lm-string": { color: "#22863a" },
  ".cm-lm-atom": { color: "#d73a49" },
  "&dark .cm-lm-comment": { color: "#7f848e" },
  "&dark .cm-lm-header": { color: "#c678dd" },
  "&dark .cm-lm-key": { color: "#61afef" },
  "&dark .cm-lm-string": { color: "#98c379" },
  "&dark .cm-lm-atom": { color: "#d19a66" },
});

/** Syntax highlighting for TOML or dotenv text. */
export function lineModeHighlight(mode: LineMode): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = buildDecorations(view, mode);
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged) {
          this.decorations = buildDecorations(update.view, mode);
        }
      }
    },
    { decorations: (value) => value.decorations },
  );
  return [plugin, lineModeTheme];
}

/**
 * TOML parse errors as CodeMirror diagnostics, underlining the offending
 * line from the parser's column. Empty text is valid.
 */
export function tomlDiagnostics(text: string): Diagnostic[] {
  if (!text.trim()) return [];
  try {
    TOML.parse(text);
    return [];
  } catch (error) {
    const err = error as { line?: number; column?: number; message?: string };
    const lines = text.split("\n");
    const lineNo = Math.min(Math.max(err.line ?? 1, 1), lines.length);
    let lineStart = 0;
    for (let i = 0; i < lineNo - 1; i += 1) lineStart += lines[i].length + 1;
    const lineText = lines[lineNo - 1] ?? "";
    const column = Math.min(
      Math.max((err.column ?? 1) - 1, 0),
      lineText.length,
    );
    const lineEnd = lineStart + lineText.length;
    // Underline from the reported column to the end of the line; an error at
    // the very end of a line marks its last character.
    const from =
      lineText.length === 0
        ? lineStart
        : Math.min(lineStart + column, lineEnd - 1);
    const to = lineText.length === 0 ? lineStart + 1 : lineEnd;
    const message = (err.message ?? "Invalid TOML").split("\n")[0];
    return [
      {
        from: Math.min(from, text.length),
        to: Math.min(to, text.length),
        severity: "error",
        message,
      },
    ];
  }
}
