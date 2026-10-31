export interface LcovRecord {
  file: string;
  linesFound: number;
  linesHit: number;
  functionsFound: number;
  functionsHit: number;
}

export interface CoverageRow {
  dir: string;
  files: number;
  linesFound: number;
  linesHit: number;
  functionsFound: number;
  functionsHit: number;
  linesPct: number | null;
  functionsPct: number | null;
}

export function parseLcov(text: string): LcovRecord[];
export function summarize(
  records: LcovRecord[],
  dirs: string[],
  root?: string,
): CoverageRow[];
export function toMarkdown(title: string, rows: CoverageRow[]): string;

export interface CoverageFloor {
  dir: string;
  min: number;
}

export interface CoverageFailure extends CoverageFloor {
  actual: number | null;
}

export function parseMin(value: string): CoverageFloor;
export function checkThresholds(
  rows: CoverageRow[],
  mins: CoverageFloor[],
): CoverageFailure[];
export function formatFailure(failure: CoverageFailure): string;
