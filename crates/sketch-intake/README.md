# Rust sketch intake

Rust SVG/DXF intake and WAZER job planning: inspect geometry in millimetres,
select a source-pinned table row, call Transmog/Swarf for typed operation drafts,
and independently recompute retained reports. See the
[OpenWAM offline project workflow](../../wazer/openwam.md),
[cutting-data runbook](../../wazer/cutting-data.md) and
[owner integration runbook](../../wazer/integration.md), and
[job simulation runbook](../../wazer/job-simulation.md). Run from Reversal:

Use the primary agent interface:

```sh
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam -- capabilities
```

The [agent runbook](../../wazer/openwam-api.md) describes typed JSON preparation,
revision, catalog, verification and fault replay requests. The future product
frontend will use Svelte/Coherence over this API. The optional
`openwam-preview-ui` binary is a retained prototype. Machine export remains disabled.

```sh
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- crates/sketch-intake/examples/plate.svg > /tmp/plate-preview.json
cargo test --locked --manifest-path crates/sketch-intake/Cargo.toml
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --plan-wazer crates/sketch-intake/examples/plate.svg desktop Aluminum 3 metric > /tmp/plate-job.json
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --verify-wazer crates/sketch-intake/examples/plate.svg /tmp/plate-job.json
```

The library API is `inspect_svg(&[u8]) -> Result<Sketch>`. It uses the established
`usvg` parser, pinned to 0.45.1, for shape conversion, local `<use>` references,
viewBox/aspect-ratio handling and cumulative transforms. No geometry kernel is
reimplemented. `dxf_intake::inspect_dxf` adds explicitly scaled planar DXF intake.
This crate has no firmware or browser dependency; owner integration uses explicit
sibling Cargo path dependencies. The lockfile fixes registry dependency resolution.

Root width and height must be explicit absolute lengths. Unitless coordinates and
px use SVG's 96 DPI convention; mm, cm, inches, points and picas are supported.
The output retains SVG's top-left origin and downward Y axis. Transformations
apply once before conversion to mm. Paths retain quadratic/cubic curves; SVG arcs
and circles become cubic curves through usvg, rather than through our own parser.
These are float32-normalized SVG curves, not exact CAD conics.

Every report binds the original input SHA-256 and reports open subpaths. It never
closes gaps automatically. Input is bounded to 1 MiB, XML nesting to 128 levels
and source/expanded segments to 100,000. Text, images, external references,
scripts, nested viewports, clipping, masking and filters fail explicitly.
Editor metadata is ignored as geometry. No browser, account or network access
is needed.

This is an inspection format, not the final CAM intermediate representation.
It describes visible path centrelines, not stroke outlines, filled regions or
material removal. SVG normalization can omit invisible/degenerate content;
the importer does not certify source completeness. It does not yet check
self-intersections, nesting, duplicate edges, clipped/out-of-bed geometry, small
features, material constraints or cutting order. `machine_output_enabled` is
always false. The separate agent API emits typed research G-code candidates;
there is no machine connection or hardware-qualified export.

## CAM boundaries

```mermaid
flowchart LR
  A[SVG / DXF / projected CAD sketch] --> B[Normalize to mm geometry]
  B --> C[Validate topology and explain repairs]
  C --> D[Assign stock, material and cut policy]
  D --> E[Kerf offsets, tabs, leads and cutting order]
  E --> F[Simulate and validate the job]
  F --> G[Versioned WAZER postprocessor]
```

SVG and bounded planar DXF intake are implemented. Native CAD sketches remain
a future adapter into the same geometry contract. Native sketch constraints belong to the CAD
owner; CAM should consume solved/projected geometry. `verk/cam-pilot` already
references `transmog-cam` and related geometry crates, but that dependency's
checkout is absent locally. Reuse that owner through an explicit interface when
it is available; do not implement another CAD kernel here.

Published owners were inspected at exact commits:

- [Transmog 48ba2fe](https://github.com/FuturePresentLabs/transmog/tree/48ba2feb40e171f639df72d3ae97d5cabfd20ef7): `transmog-core::ir::Feature::Sketch`, `PlaneRef` and `SketchSegment` are the existing sketch boundary; kernel/OCCT crates own solid construction. `SketchSegment::Spline` describes interpolated points, so SVG Bézier control points cannot be copied into it as an equivalent curve. A conversion needs an explicit fitting/flattening tolerance and geometric error tests. The inspected published tree has no `transmog-cam` crate; local unpublished versions may differ.
- [Swarf baseline aea1287](https://github.com/FuturePresentLabs/swarf/tree/aea128794c0aa6d238a01db96a1423d7f3f12d78): `src/ast`, `src/codegen` and `src/post` own the milling compiler. Local commit `08ddcfb1d7ecff2e6a36b4760e5a385cc6fd40cb` extends `crates/swarf-waterjet` with a typed observed WAZER dialect and research postprocessor. Reversal invokes this owner over actual Transmog profiles. [Candidate generation and verification](../../wazer/wazer-postprocessor.md) remain explicitly unqualified for hardware; milling spindle/coolant defaults and feeds are not used.

Reversal owns compatibility observations, fixtures and this initial preview
adapter. Transmog owns reusable geometry; Swarf owns manufacturing compilation
and postprocessing. The local owner integration is implemented through narrow
Rust interfaces; the remaining qualified CAM/postprocessor stages are tracked
separately.

The next geometry boundary must preserve contour identity, holes, open versus
closed intent, explicit tolerance in mm, source provenance and reported repairs.
Endpoint snapping must be a requested operation with a before/after receipt.
Kerf is a material/machine parameter, not SVG stroke width. Cut-side selection,
hole-first ordering, piercing, leads and tabs need deterministic geometric tests.

WAZER output requires a dedicated, versioned postprocessor. Public client code
contains G-code-generation and machine-specific command logic; ordinary generic
G-code is not evidence of compatibility. Capture known WAM exports for synthetic
SVG fixtures, bind machine model/settings/material and exact file hashes, then
compare our output and simulation against those fixtures. Account/API access
should remain optional to the local geometry core. Do not mark exports verified
until these tests exist and pass. Follow-up work is tracked in Beads
`reversal-l3u`, `reversal-uhn` and `reversal-3ym`.
