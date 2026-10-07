# OpenWAM agent API and CLI

The primary OpenWAM interface is a stateless Rust API with a JSON CLI transport.
Agents and future clients use the same typed operations and owner validation.
The library is `reversal_sketch_intake::openwam_api`; `dispatch(bytes)` performs
bounded request decoding and uses the captured catalog. `execute(request, catalog)`
supports in-process callers with an explicitly supplied, validated catalog.

## Discover and execute

```sh
cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam
target/debug/openwam capabilities > /tmp/openwam-capabilities.json
jq '.prepare_example' /tmp/openwam-capabilities.json > /tmp/openwam-prepare.json
target/debug/openwam request /tmp/openwam-prepare.json > /tmp/openwam-prepared.json
jq '.result.project' /tmp/openwam-prepared.json > /tmp/openwam-project.json
jq -n --slurpfile cap /tmp/openwam-capabilities.json --slurpfile project /tmp/openwam-project.json '{schema:$cap[0].request_version,request:{operation:"verify",project:$project[0]}}' > /tmp/openwam-verify.json
target/debug/openwam request /tmp/openwam-verify.json
```

`openwam request` and `openwam request -` read one JSON request from stdin.
`openwam request filename.json` reads a bounded file. The process writes one JSON
document to stdout, with no banners/progress mixed into the stream. Exit 0 means
successful processing; exit 2 accompanies a structured error. An output I/O
failure exits 3 and reports it to stderr because the JSON output channel failed.

Capabilities include generated JSON Schemas for request and response Rust types,
protocol versions, byte limits and a valid preparation example. Schema generation
is derived from types rather than a separately maintained frontend definition.
Schemas describe structure; owner validation additionally checks source geometry,
material points, full project consistency and replay consistency.

## Request and response contract

Requests use `reversal.openwam-request.v1`:

```json
{
  "schema": "reversal.openwam-request.v1",
  "request": {
    "operation": "catalog",
    "model": "pro",
    "material": "Aluminum",
    "units": "metric"
  }
}
```

| Operation | Inputs | Result |
|---|---|---|
| `catalog` | Optional model, exact material name, unit system | Rows, catalog digest and source provenance; unsupported/anomalous points remain visible |
| `prepare` | Exact UTF-8 source, `svg`/`dxf`, explicit settings | Portable independently recomputable project |
| `revise` | Retained project and new settings | New project; original is first verified and never mutated |
| `verify` | Project | Source/owner recomputation receipt |
| `preview` | Project | Standalone SVG media type and content derived from verified operations |
| `plan_cam` | Project and exact per-contour cut intent | Retained independent compensated CAM artifact with tabs and waste-side leads |
| `verify_cam` | CAM artifact | Source/intent/table/owner plan recomputation receipt |
| `preview_cam` | CAM artifact | Standalone SVG of independently verified compensated cuts, uncut gaps and rapids |
| `simulate_cam` | CAM artifact and explicit fault kind/time | CAM-bound synthetic replay executing compensated cut spans and non-cutting travel |
| `verify_cam_simulation` | CAM artifact and retained replay | Independent source/intent/plan/fault replay recomputation |
| `research_cam_gcode` | CAM artifact and explicit `historical_wam16` profile | CAM-bound multi-span research candidate; controller qualification stays false |
| `verify_cam_gcode` | CAM artifact and candidate | Independent source/intent/table/postprocessor regeneration |
| `simulate` | Project and explicit fault kind/time | Synthetic motion/controller/plant replay |
| `verify_simulation` | Project and retained replay | Independent recomputation receipt |
| `inspect_gcode` | G-code text | Typed observed-dialect program, canonical serialization, digest and feed/dwell summary |
| `research_gcode` | Project and explicit `historical_wam16` or `wam24_pro_presegmented` profile | Source/table/project-bound research candidate; machine eligibility stays false |
| `verify_research_gcode` | Project and candidate | Independent regeneration receipt; controller qualification stays false |
| `compare_gcode` | Reference/candidate text and explicit coordinate/feed/dwell tolerances | Bounded ordered command differences, modal feed comparison and separate equality levels |
| `compare_research_gcode` | Project, profile, reference text and tolerances | Rebuilt candidate compared to reference with project/source/table/stream bindings |
| `verify_research_comparison` | Project, reference text and comparison | Full regeneration/recomparison receipt; reference producer and source/settings pairing remain unestablished |

Settings contain `model`, `material`, `units`, `thickness`, `tolerance_mm` and
`placement_xy_down_mm`. Thickness uses the chosen metric/imperial table system;
placement and tolerance always use mm. No GUI table indices or implicit selections
enter the API. Catalog material filtering is case-insensitive exact matching.

Responses use `reversal.openwam-response.v1`. Success has `status: "ok"` and
`result`, whose `kind` identifies the output. Failure has `status: "error"` and
`error: {code, message}`. Stable codes distinguish request shape/version, I/O,
budgets, catalog, project, planning, simulation and replay failures. Agents should
branch on codes, not parse human messages. Research operations additionally use
`gcode_invalid`, `postprocessing_failed`, `invalid_candidate`, `comparison_failed`
and `invalid_comparison`. Independent contour CAM uses `cam_invalid` for rejected
geometry, intent or artifacts. Both responses keep
`machine_output_enabled: false`. A successfully generated fault replay can have
`outcome: "faulted"`; transport success does not mean the simulated job completed.

Unknown request fields/operations/schema versions fail. Request input is bounded
to project capacity plus 2 MiB, source to 1 MiB and project to 64 MiB; response
budget is twice the request budget. There is no unbounded streaming/session
accumulation, account login or runtime network access. Save the original request,
response and selected project/replay as local artifacts for your research loop.

## Observe, simulate, verify

The manual/agent sequence is deterministic: inspect capabilities and the catalog,
prepare from original source and explicit settings, verify the project, simulate
one explicit scenario, independently verify its replay, then compare the recorded
outcome/frames to the next research question. Injecting `enclosure_open` at
10,000 ms into the plate example records a fault at that exact boundary. A forged
frame fails `verify_simulation`; a changed project fails before replay.

Use `fault: {"kind":"none","at_ms":0}` for nominal replay. Other supported
kinds are generated in the discovery schema from the existing simulator enum.
An unreachable fault time is an error rather than an unexercised successful test.

The API returns artifacts and observations. It does not autonomously choose
experiments, repair firmware, flash devices or certify hardware. Current contour,
kerf, ordering and postprocessor limits remain in [openwam.md](openwam.md).

The [Rust research postprocessor](wazer-postprocessor.md) extends this loop with
candidate generation, typed dialect inspection and independent verification.
The [comparison harness](wazer-parity.md) adds explicit tolerances, bounded
differences and report verification without inferring reference provenance.

The [independent contour CAM runbook](independent-cam.md) adds our own SVG import,
kerf compensation, holes-first ordering, explicit starts, uncut tabs and leads.
Its retained CAM artifact is independently verified. The [CAM loop](cam-loop.md)
connects it to fault replay and multi-span historical research exports without
changing the older project-only operations.

The [CLI demonstration receipt](openwam-api-demonstration.json) records retained
request/response hashes, independently verified project and replay, and nominal
completion of all 125 plate operations in 207,158 virtual ms. All 42 tests,
all-target Clippy with the experimental UI, and a headless CLI type check passed.
Tests also exercise revisions, explicit fault replay, forged artifacts and
structured errors through the actual CLI process.

## Frontend boundary

The previous server-rendered prototype is retained only as the optional
`openwam-preview-ui` experiment; it is no longer the primary `openwam` command.
No HTTP/MCP transport for this agent API is registered yet. Add a thin transport
when its consumer needs it, calling the same API and translating structured
errors without copying CAM logic.

The intended product frontend is Svelte with Coherence components/tokens.
Coherence's local `../fpl/coherence` checkout documents Svelte consumption and
the `dist/coherence-product.css` product profile. Consume that owner's generated
artifacts; do not create another embedded CSS design system here. Frontend work
follows the verified agent interface and consumes its generated schemas.

The current Pro service profile supports one closed, supplied presegmented path,
rejects Desktop projects and unsupported geometry, and retains machine eligibility
as false. See [verified service scope](wam-pro-parity.md).
