/**
 * live 配置写入预览的逐行 diff（并排视图）。
 *
 * 先裁掉相同的首尾行，再对中间部分做 LCS；中间部分过大时退化为整段替换，
 * 避免超大文件的 O(n·m) 开销。
 */

export interface DiffLine {
  lineNo: number;
  text: string;
}

export type DiffRowKind = "same" | "removed" | "added" | "changed";

/** 并排视图的一行：左侧为当前内容，右侧为保存后的内容 */
export interface DiffRow {
  kind: DiffRowKind;
  left?: DiffLine;
  right?: DiffLine;
}

/** 折叠后的一段：要么是要显示的行，要么是被省略的未改动行数 */
export type DiffSegment =
  | { type: "rows"; rows: DiffRow[] }
  | { type: "skipped"; count: number };

/** 中间段 LCS 表的格数上限（约 4M 个 Uint32，16MB） */
const MAX_LCS_CELLS = 4_000_000;

export function splitLines(text: string | null | undefined): string[] {
  if (!text) return [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  // 末尾换行不产生额外的空行
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

type Op = { kind: "same" | "removed" | "added"; text: string };

function diffOps(a: string[], b: string[]): Op[] {
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) {
    start++;
  }
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }

  const ops: Op[] = [];
  for (let i = 0; i < start; i++) ops.push({ kind: "same", text: a[i] });

  const midA = a.slice(start, endA);
  const midB = b.slice(start, endB);
  const n = midA.length;
  const m = midB.length;

  if (n > 0 && m > 0 && (n + 1) * (m + 1) <= MAX_LCS_CELLS) {
    // lcs[i][j] = midA[i..] 与 midB[j..] 的 LCS 长度
    const width = m + 1;
    const lcs = new Uint32Array((n + 1) * width);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        lcs[i * width + j] =
          midA[i] === midB[j]
            ? lcs[(i + 1) * width + j + 1] + 1
            : Math.max(lcs[(i + 1) * width + j], lcs[i * width + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
      if (midA[i] === midB[j]) {
        ops.push({ kind: "same", text: midA[i] });
        i++;
        j++;
      } else if (lcs[(i + 1) * width + j] >= lcs[i * width + j + 1]) {
        ops.push({ kind: "removed", text: midA[i++] });
      } else {
        ops.push({ kind: "added", text: midB[j++] });
      }
    }
    while (i < n) ops.push({ kind: "removed", text: midA[i++] });
    while (j < m) ops.push({ kind: "added", text: midB[j++] });
  } else {
    for (const text of midA) ops.push({ kind: "removed", text });
    for (const text of midB) ops.push({ kind: "added", text });
  }

  for (let i = endA; i < a.length; i++) ops.push({ kind: "same", text: a[i] });
  return ops;
}

/** 计算并排 diff 行；相邻的删除与新增按顺序配对为 "changed" 行 */
export function diffLines(
  before: string | null | undefined,
  after: string | null | undefined,
): DiffRow[] {
  const ops = diffOps(splitLines(before), splitLines(after));
  const rows: DiffRow[] = [];
  let leftNo = 1;
  let rightNo = 1;

  let k = 0;
  while (k < ops.length) {
    const op = ops[k];
    if (op.kind === "same") {
      rows.push({
        kind: "same",
        left: { lineNo: leftNo++, text: op.text },
        right: { lineNo: rightNo++, text: op.text },
      });
      k++;
      continue;
    }
    const removed: string[] = [];
    const added: string[] = [];
    while (k < ops.length && ops[k].kind !== "same") {
      (ops[k].kind === "removed" ? removed : added).push(ops[k].text);
      k++;
    }
    const pairs = Math.max(removed.length, added.length);
    for (let p = 0; p < pairs; p++) {
      const left =
        p < removed.length ? { lineNo: leftNo++, text: removed[p] } : undefined;
      const right =
        p < added.length ? { lineNo: rightNo++, text: added[p] } : undefined;
      rows.push({
        kind: left && right ? "changed" : left ? "removed" : "added",
        left,
        right,
      });
    }
  }
  return rows;
}

export function hasChanges(rows: DiffRow[]): boolean {
  return rows.some((row) => row.kind !== "same");
}

/** 只保留改动行及其前后 `context` 行，其余未改动行折叠为计数 */
export function collapseUnchanged(rows: DiffRow[], context = 3): DiffSegment[] {
  const keep = new Array<boolean>(rows.length).fill(false);
  rows.forEach((row, index) => {
    if (row.kind === "same") return;
    const from = Math.max(0, index - context);
    const to = Math.min(rows.length - 1, index + context);
    for (let i = from; i <= to; i++) keep[i] = true;
  });

  const segments: DiffSegment[] = [];
  let index = 0;
  while (index < rows.length) {
    const kept = keep[index];
    let end = index;
    while (end < rows.length && keep[end] === kept) end++;
    segments.push(
      kept
        ? { type: "rows", rows: rows.slice(index, end) }
        : { type: "skipped", count: end - index },
    );
    index = end;
  }
  return segments;
}
