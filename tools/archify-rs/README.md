# archify-rs

A Rust compiler for typed-JSON diagram specifications. It reads the same
`architecture`, `sequence` and `workflow` documents the archify skill
authors, resolves them into geometry, runs a battery of composition checks,
and renders a self-contained HTML page with an inline SVG — light/dark
theme, pan/zoom, guided views, legend, conclusion cards, and SVG/PNG export
that resolves CSS variables so the exported file stands alone.

No runtime dependencies: one binary, one HTML file out.

```
cargo build --release
./target/release/archify-rs doctor
./target/release/archify-rs validate architecture spec.json --quality showcase --json
./target/release/archify-rs deliver   architecture spec.json out.html --json
./target/release/archify-rs demo ./examples-out
```

## Commands

| Command | Behaviour | Exit |
|---|---|---|
| `validate <type> <spec>` | Parse, lay out, run every check; write nothing | 0 pass / 1 fail / 2 usage |
| `render <type> <spec> <out.html>` | Render even when checks fail (diagnostics still print) | as above |
| `deliver <type> <spec> <out.html>` | Checks must pass; writes atomically (a failed delivery leaves any previous file untouched); prints SHA-256 and byte counts for spec and artifact | as above |
| `demo <dir>` | Writes the bundled example specs and their rendered pages | |
| `doctor` | Lists supported types, profiles and checks | |

`<type>` is `architecture`, `sequence`, `workflow` or `auto` (reads
`diagram_type` from the document). `--json` prints a receipt shaped like
archify's (`checks`, `composition`, `diagnostics`, or the delivery
`specification`/`artifact` digests).

## Checks

| Check | What it enforces |
|---|---|
| `references` | every `from`/`to`/`lane`/`wraps`/`focus` id resolves |
| `finite_svg` | no NaN/∞ coordinates |
| `orthogonal_arrows` | every route segment is axis-aligned, no sub-8px micro-segments |
| `node_overlap` | node rectangles never intersect |
| `label_fit` | node label and sublabel fit inside the node (accent bar and padding included) |
| `relationship_crossings` | no route passes through an unrelated node |
| `relationship_corridors` | two routes never share a collinear run (ambiguous corridor) |
| `label_route_clearance` | relationship labels never touch a node, another label, or come within 4px of another route |
| `viewbox_containment` | nodes, containers and labels stay inside the viewBox |
| `timeline` | sequence messages and activations stay in the readable band (`160 … height − 83`) |
| `desktop_readability` | *showcase only* — node text projects to ≥ 6px in a 930px reading column |

Structural rules (schema version, id syntax, uniqueness, minimum viewBox,
unknown fields) are enforced at parse time and reported as usage errors.

## Layout rules

* **Architecture** — explicit `pos`/`size` (or `layout.grid` with
  `row`/`col`). Endpoint sides are inferred from relative placement unless
  `fromSide`/`toSide` is authored; routes are straight, L or Z. Ports that
  share a side are spread evenly and ordered by the far endpoint; when only
  one end of a facing pair was spread, the other end slides onto its axis so
  the run stays straight. Boundaries wrap their members with padding.
* **Sequence** — participants in fixed 86px boxes with 108px gaps, or
  `meta.column_fit: "spread"` to derive wider boxes from the viewBox.
  Messages are horizontal arrows at their `y`; `return`/`dashed` variants
  render dashed; activations are bars on the lifeline; segments are bands.
* **Workflow** — lanes stacked vertically (exception lanes styled red),
  six columns whose width follows the widest node placed in them, phases
  as bands above the lanes, groups as dashed frames inside a lane. Same-lane
  adjacent edges are straight; same-lane non-adjacent edges use a channel
  under the lane; cross-lane edges drop from bottom to top. `route`
  (`straight`/`drop`), `channelX`/`channelY`, `via`, and label offsets are
  honoured.

Labels default to the longest segment: above a horizontal run, beside a
vertical run (flipping to the left near the right edge of the node field).
`labelSegment`, `labelDx`, `labelDy` and `labelAt` adjust them.

## Tests

```
cargo test
cargo clippy --all-targets
cargo fmt --check
```

`tests/fixtures/` holds three real specs that must pass the full showcase
battery and render deterministically, plus two deliberately broken specs
that must fail for the documented reason.
