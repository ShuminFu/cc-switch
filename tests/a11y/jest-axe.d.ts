// jest-axe ships without types; declare only the surface the a11y tests use.
declare module "jest-axe" {
  export interface AxeViolation {
    id: string;
    help: string;
    nodes: Array<{ html: string }>;
  }

  export interface AxeResults {
    violations: AxeViolation[];
  }

  export function axe(
    html: Element | string,
    options?: { runOnly?: string[] | { type: string; values: string[] } },
  ): Promise<AxeResults>;
}
