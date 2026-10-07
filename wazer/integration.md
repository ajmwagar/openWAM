# SVG/DXF → Transmog → Swarf waterjet drafts

Reversal now calls the actual Transmog core types and a Swarf-owned Rust waterjet
library. It does not fork a geometry kernel or reuse milling/spindle output.
The loop runs original input → mm inspection → exact WAZER table selection →
Transmog line profiles → Swarf typed operations → independent recomputation.

## Owner boundaries and setup

The sibling checkouts are explicit Cargo path dependencies, matching the local
FPL layout:

| Owner | Checkout relative to Reversal | Inspected/local revision |
|---|---|---|
| Transmog | `../fpl/transmog` | `48ba2feb40e171f639df72d3ae97d5cabfd20ef7` |
| Swarf | `../fpl/swarf` | `8b7d4da15627981b48b65a3e788cbf6faad91a22` |

Swarf's new `crates/swarf-waterjet` is a lightweight library with its own manifest.
It consumes Transmog `SketchSegment::Line` profiles and compiles deterministic
Rapid/Pierce/Cut/Stop operations. The Swarf change is a local commit; it must be
shared before another machine can reproduce this revision. No remote push was
performed. Missing sibling checkouts fail during Cargo dependency resolution.

## Run and verify

From Reversal's root:

```sh
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --integrate-wazer crates/sketch-intake/examples/plate.svg pro Aluminum 3 metric 0.02 0 0 > /tmp/plate-integration.json
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --verify-integration crates/sketch-intake/examples/plate.svg /tmp/plate-integration.json
```

Arguments after the unit system are curve tolerance in mm, stock-placement X,
and stock-placement Y down, both in mm. There is no automatic positioning. SVG
top-left/down coordinates are reflected to CAD XY with Y negative down for the
Transmog/Swarf boundary. The original planning estimate retains the unplaced
sketch; Swarf validates the explicitly placed line profiles against machine bounds.

For the retained manufacturer DXF, an explicit placement keeps its negative
CAD coordinates in the cutting area:

```sh
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --integrate-wazer wazer/private/pro-reference/gauge-block.dxf pro Aluminum 3 metric 0.02 95 115 > /tmp/gauge-integration.json
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --verify-integration wazer/private/pro-reference/gauge-block.dxf /tmp/gauge-integration.json
```

This demonstration deliberately selects the published **3 mm** table point.
The vendor G-code describes **4 mm** stock and a different input filename; this
demonstration does not claim geometry, cutting settings or timing equality to it.

Each integration report retains the original sketch/table binding, typed Transmog
features, explicit placement/tolerance, Swarf request and operations. Verification
reparses the original source and rebuilds both stages; forged operations, changed
input, extra fields and altered output eligibility fail.

## Geometry support and limits

SVG quadratics/cubics are subdivided with `kurbo`. The adapter bounds each leaf's
Bezier-to-chord deviation using the control polygon of the difference from the
degree-elevated chord. This is an all-parameter bound, independently tested with
sampled distances. The maximum retained bound is reported. It applies to
normalized curves; usvg's arc approximation and float32 transforms remain
additional upstream errors. Bezier controls are never mislabeled as Transmog's
interpolated spline points.

DXF uses `dxf` 0.6.2, with explicit `INSUNITS` (mm/in/cm/m), planar LINE, ARC,
CIRCLE and straight LWPOLYLINE support. Circular subdivision uses a sagitta bound
at the requested tolerance. Binary DXF, missing/unknown units, INSERT, SPLINE,
bulges, thick/elevated/non-XY entities and hidden/frozen geometry fail. Validated
paper-space VIEWPORT records are explicitly counted as layout metadata before
comparing parsed geometry counts. Unknown entities cannot silently disappear.

Separate DXF entities remain separate contours. No gaps are snapped, branches
joined, holes classified or contours reordered. The vendor gauge demonstration
therefore has 18 raw entity contours; its pierce/time estimate is a per-entity
draft, not a verified cutting sequence. The next geometry stage must qualify
joining, topology, kerf offsets, leads, tabs and hole ordering.

Every result keeps `machine_output_enabled = false`. Swarf now owns a
[typed WAZER research postprocessor](wazer-postprocessor.md), with candidate
generation and independent verification through the OpenWAM agent API. The
[official firmware/reference receipt](pro-firmware-acquisition.json)
pins Pro V2.32 and welcome files tagged WAM 1.6; installed firmware and current-WAM
compatibility remain unknown. `reversal-l3u` and `reversal-uhn` retain the remaining
geometry and controller work.

Validation: 25 Reversal tests (7 integration, 11 cutting-data, 7 SVG) and two
Swarf compiler tests; Clippy and formatting checks. Real SVG and manufacturer DXF
reports were independently recomputed. Retained demonstration digests are in
[integration-acquisition.json](integration-acquisition.json).
