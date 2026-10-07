# WAZER comparison harness and fixture acquisition

Swarf owns `swarf_waterjet::wazer_compare`: a deterministic Rust comparison of
bounded observed command streams. OpenWAM exposes it through `compare_gcode`,
`compare_research_gcode` and `verify_research_comparison`. This measures output
agreement; it does not establish that an arbitrary reference came from WAM or
that its source/settings match our project.

## Agreement levels

| Field/verdict | Meaning |
|---|---|
| `byte_identical` | Every input byte is identical, including comments and line endings |
| `commands_exactly_equal` | Parsed commands are identical; formatting/comments are excluded, explicit versus omitted feed words still differ |
| `equivalent_observed_trace` | Aligned command kinds, coordinates, resolved modal feeds, durations and metadata agree under the explicit tolerances; all cutting feeds are known |
| `unresolved_initial_feed` | No compared field differs, but at least one cutting move has no established feed |
| `different` | At least one compared field or command count differs |

Comparison uses zero-based **command index**, never source line number. Inserting
one command can create later alignment differences. It does not reorder paths,
fit translations, resample curves or infer geometric equivalence. Coordinates use
Euclidean distance in mm, feeds absolute mm/min difference and dwell/header-pierce
values absolute seconds difference. Tolerances are required and must be finite
and nonnegative. Metadata is compared as literal labels; displayed job time is
metadata rather than verified physical duration. Both input summaries retain known
feed cutting time, unresolved feeds and dwell totals.

All differences are counted. The first 64 category differences include commands,
indices and effective feeds; `differences_truncated` signals additional detail.
Counts may include multiple categories for one command. Each input retains the
existing 16 MiB/1,024-byte-line/400,000-command limits. Invalid or unsupported
commands fail instead of disappearing from the comparison. Errors identify which
input failed parsing.

## Agent/manual runbook

```sh
cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam
crates/sketch-intake/target/debug/openwam capabilities
jq -n --rawfile reference /tmp/reference.gcode --rawfile candidate /tmp/candidate.gcode '{schema:"reversal.openwam-request.v1",request:{operation:"compare_gcode",reference_gcode:$reference,candidate_gcode:$candidate,tolerances:{coordinates_mm:0,feed_mm_min:0,dwell_seconds:0}}}' > /tmp/compare-streams.json
crates/sketch-intake/target/debug/openwam request /tmp/compare-streams.json
```

To rebuild our candidate directly from a saved project and compare it:

```sh
jq -n --slurpfile project /tmp/project.openwam.json --rawfile reference /tmp/reference.gcode '{schema:"reversal.openwam-request.v1",request:{operation:"compare_research_gcode",project:$project[0],profile:"historical_wam16",reference_gcode:$reference,tolerances:{coordinates_mm:0,feed_mm_min:0,dwell_seconds:0}}}' > /tmp/compare-project.json
crates/sketch-intake/target/debug/openwam request /tmp/compare-project.json > /tmp/compared.json
jq -n --slurpfile request /tmp/compare-project.json --slurpfile response /tmp/compared.json '{schema:"reversal.openwam-request.v1",request:{operation:"verify_research_comparison",project:$request[0].request.project,reference_gcode:$request[0].request.reference_gcode,comparison:$response[0].result.comparison}}' > /tmp/verify-comparison.json
crates/sketch-intake/target/debug/openwam request /tmp/verify-comparison.json
```

A mismatch is a successful observation (`status: ok`, verdict `different`). Invalid
code/tolerances return `comparison_failed`; forged research reports return
`invalid_comparison`. Reverification rebuilds source, tables and candidate, then
recomputes using the report's explicitly retained tolerances. It checks hashes,
all differences and all flags. It does not authenticate the caller's original
choice of tolerances or prove the reference producer. Even identical streams keep
`source_settings_equivalence_established`, `machine_output_enabled` and
`controller_qualified` false.

## First actual same-input fixture

Acquire a WAM export together with exact original SVG/DXF bytes and recorded
settings. Retain model, material identity, thickness, cut quality/mode, placement,
scale/rotation, cut side, leads, tabs, starts, group/segment order, WAM version,
export provenance and any controller target. Missing settings stay unresolved.

Start with one straight, closed contour at a supported exact catalog thickness,
using centerline intent to isolate serialization from offsets and topology. The
same source bytes must be imported into both systems; use explicit placement.
Record WAM's actual feeds and pierce settings rather than assuming that its quality
recipe equals the public table. Current Rust output has no kerf/leads/tabs/corner
policy, so those options need separate fixtures and owner implementations.

The prepared [square SVG](fixtures/parity-square.svg) has one nominal 30 mm closed
square in a 50 mm viewport, with source vertices at (10,10) through (40,40).
Its retained OpenWAM project is `private/wazer-parity/square.openwam.json`, selected
for Pro/Aluminum/metric/3 mm, tolerance 0.02 mm and placement [0,0]. The public table
supplies 108 mm/min feed and 6 seconds pierce. Project and candidate independently
verify; the candidate has one path and four cutting moves. This fixture is ready
for WAM capture but has no vendor export yet. Record WAM's actual material identity,
quality, centerline intent, placement and disabled/enabled process options; do not
assume that loading the file establishes equivalent settings.

The captured current client is 2.4.0; the only implemented output profile is
historical 1.6. Do not rewrite the reference version to make a comparison pass.
Begin with zero tolerances, then document any second run with tolerances justified
by measured formatting/flattening error. Keeping an independent reference matters:
a self comparison or serializer round trip cannot test toolpath-generation parity.

## Results and remaining evidence

[The receipt](wazer-parity-demonstration.json) records actual CLI runs:

- Official nozzle export versus its Rust canonical serialization: commands agree,
  all 309 feeds are resolved, bytes differ through comments/formatting.
- Official gauge export versus canonical serialization: commands agree, but one
  initial feed is unresolved, so the trace verdict remains indeterminate.
- Changing only the first nozzle feed from 100 to 90 produces exactly one feed
  difference and a `different` verdict.
- Retained 8HP candidate self comparison independently re-verifies its project and
  stream bindings; this is a recomputation control, not a WAM parity fixture.
- Preparing the retained gauge DXF at 4 mm fails because there is no exact published
  Aluminum/Pro/metric/4 mm recipe. No substitute thickness or interpolation is used.

There is **no completed same-input WAM CAM parity run**. Both official exports
specify 4 mm Aluminum-PRO and have unverified relationships to the downloaded
DXFs. Browser discovery reports no available browser in this session. Acquisition
and source/settings pairing remain `reversal-7dd`; broader process/controller
qualification remains `reversal-uhn`. No hardware transfer, cuts or flashing.

The [local Node oracle](wam-node-oracle.md) now executes the original WAM 2.4.0
math/serializer/Pro generator on an explicit presegmented square fixture. It
demonstrates concrete corner-feed, repeated-endpoint, metadata and time differences
against Rust. It excludes upstream SVG import/segmentation and library selection,
so it does not close the full CAM pairing gap described above.
