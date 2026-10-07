# Independent OpenWAM contour CAM

OpenWAM now imports SVG through its own offline Rust pipeline and plans contour
cuts through Transmog and Swarf. WAM's importer, account and JavaScript are not
required. The captured WAZER tables still supply material feed, pierce time,
stock dimensions and nominal kerf. The Pro fixture uses the published 1.1 mm
nominal kerf; an explicit override supports a separately measured kerf.

This is the first bounded contour CAM implementation, not a qualified WAZER
postprocessor. Its outputs are planning operations and a standalone SVG preview.
Every artifact and response keeps `machine_output_enabled: false`.

## Ownership and source identity

Reversal's existing `usvg` normalizer handles physical SVG dimensions, viewBox,
transforms, local reuse and supported shapes. Curves are flattened with the
explicit project tolerance in the Transmog bridge. Inkscape namedview metadata
is ignored. External resources, scripts, ambiguous dimensions and unsupported
rendering features fail explicitly; paths are never silently snapped closed.

Unique author IDs remain unchanged. Anonymous contours receive document-local
`svg-contour-N` identifiers. Compound paths receive `author-id/subpath-N`.
Generated/literal ID collisions fail with a request to rename the source.
The original SVG is retained verbatim in the project, including author IDs.
Identifiers are repeatable for identical source, not persistent across arbitrary
document edits. Earlier compound-path reports need regeneration; saved projects
are never silently migrated to different contour identities.

Transmog owns polygon validation, containment, arc-length slicing and miter
offsets. Swarf owns cut intent, compensation, ordering, tabs, leads and draft
operations. Reversal exposes those owners through the existing agent API; no
parallel geometry implementation or frontend schema is introduced.

## Explicit cut intent

Each imported contour must have exactly one entry in `intent.contours`:

```json
{
  "source_id": "svg-contour-0",
  "side": "outside",
  "start_fraction": 0,
  "tabs": [{"center_fraction": 0.125, "width_mm": 3}],
  "lead_in_mm": 2,
  "lead_out_mm": 2
}
```

`outside` expands the tool centerline by half the kerf; `inside` contracts it.
The explicit side must agree with even-odd nesting of selected boundaries.
Deeper features are cut first, with document order breaking ties. `centerline`
applies no compensation and requires zero leads. `no_cut` excludes an annotation
from material boundaries and cutting; it can be open and requires no tabs/leads.
Artwork fill and stroke do not decide which material to keep.

Fractions are arc-length positions on the compensated closed contour, with
zero at its original seam. `start_fraction` changes the seam. Tab fractions
stay bound to that original seam; output intervals are measured from the chosen
start. Fractions must be in `[0,1)`. Each tab removes a centerline interval of
`width_mm`, rather than issuing a cutting move through a bridge. Width must
exceed kerf, and tabs must leave cut spans longer than kerf. Overlaps and tabs
crossing the chosen start fail explicitly; choose another start or tab location.
Widths represent nominal centerline gaps, not measured bridge holding strength.

Every cut span, including each span following a tab, gets its own requested
perpendicular lead-in and lead-out on the waste side. The planner checks these
segments against retained material boundaries with a half-kerf tool envelope.
Unsafe leads are rejected, not shortened silently. Each span receives its own
rapid, pierce, cut and stop sequence. Estimated time includes extra pierces and
lead lengths; pressure, abrasive dynamics and corner quality remain unqualified.

## Agent API and manual runbook

`plan_cam` accepts a previously prepared project and typed intent.
`verify_cam` and `preview_cam` accept the returned artifact, schema
`reversal.openwam-cam.v1`. It retains the project, intent, canonical project/table
digests and derived plan. Verification reconstructs geometry and operations from
source/intent/tables and rejects altered outputs or authority. Its generated
request schema is available through `openwam capabilities`.

Run from the OpenWAM repository. Its minimal owner dependencies are pinned in `vendor/`:

```sh
cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam
mkdir -p wazer/private/independent-cam-1
jq -n --rawfile source wazer/fixtures/cam-panel.svg \
  '{schema:"reversal.openwam-request.v1",request:{operation:"prepare",source_text:$source,source_format:"svg",settings:{model:"pro",material:"Aluminum",units:"metric",thickness:3,tolerance_mm:0.02,placement_xy_down_mm:[0,0]}}}' \
  > wazer/private/independent-cam-1/prepare.request.json
target/debug/openwam request wazer/private/independent-cam-1/prepare.request.json > wazer/private/independent-cam-1/prepare.response.json
jq -n --slurpfile prepared wazer/private/independent-cam-1/prepare.response.json \
  --slurpfile intent wazer/fixtures/cam-panel.intent.json \
  '{schema:"reversal.openwam-request.v1",request:{operation:"plan_cam",project:$prepared[0].result.project,intent:$intent[0]}}' \
  > wazer/private/independent-cam-1/plan.request.json
target/debug/openwam request wazer/private/independent-cam-1/plan.request.json > wazer/private/independent-cam-1/plan.response.json
jq -n --slurpfile planned wazer/private/independent-cam-1/plan.response.json \
  '{schema:"reversal.openwam-request.v1",request:{operation:"verify_cam",artifact:$planned[0].result.artifact}}' \
  > wazer/private/independent-cam-1/verify.request.json
target/debug/openwam request wazer/private/independent-cam-1/verify.request.json > wazer/private/independent-cam-1/verify.response.json
jq '.request.operation="preview_cam"' wazer/private/independent-cam-1/verify.request.json \
  > wazer/private/independent-cam-1/preview.request.json
target/debug/openwam request wazer/private/independent-cam-1/preview.request.json > wazer/private/independent-cam-1/preview.response.json
jq -r '.result.content' wazer/private/independent-cam-1/preview.response.json \
  > wazer/private/independent-cam-1/preview.svg
```

The preview uses the full stock dimensions: grey source boundaries, cyan cutting
moves, dashed grey rapids and orange pierce points on a white background. Open
annotations remain visible as source context, without being cut.

The [panel fixture](fixtures/cam-panel.svg) combines an unnamed outline, two
compound rectangular openings, a round opening and an open annotation. Its
[intent](fixtures/cam-panel.intent.json) adds two 3 mm tabs and leads. The actual
CLI preparation/planning/verification produced four paths, six separately pierced
cut spans and 108 operations. The [receipt](independent-cam-demonstration.json)
records input/output hashes and retained plan details. The same fixture is a
CLI regression test, including deterministic repetition, independent verification,
preview annotations and an explicit kerf override.

## Supported geometry and integration limits

Selected cutting paths must be simple closed flattened polygons. The first
implementation is bounded to 128 contours, 2,048 total selected vertices and
32 tabs per contour. Crossing/touching boundaries, collapsed/reversed edges,
offsets requiring topology splitting and miters longer than four times the
offset are rejected. Concave polygons can work within those checks; general
artwork repair, Boolean unions and topology-changing offsets are not implemented.
SVG normalization uses the existing library's numeric precision; the polygon
unit tests independently check analytic dimensions rather than claiming bit-exact
SVG geometry.

CAM artifacts are separate from existing project/research-G-code artifacts.
The [CAM loop](cam-loop.md) feeds verified CAM moves into the shared simulator
and historical research postprocessor. Multiasset placement, automatic tab selection, alternative
lead shapes and qualified controller export remain tracked integration work.
The future Svelte/Coherence client can consume this API and generated intent
schema; no ad-hoc UI was added.
