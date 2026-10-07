# legion-of-bom panel to OpenWAM

The first real owner-to-owner fixture is the existing 8HP panel: 40.64 × 128.5 mm,
2 mm nominal thickness, two oval mounting slots and three component openings.
Legion-of-bom's new `panel_cut` library and `lob-panel-cut` CLI export one closed
SVG path per opening, followed by the outline. No cutout geometry is reconstructed
in Reversal. This avoids the legacy DXF unit/fragmentation issues without changing
the existing DXF or enclosure drill templates.

The [retained owner artifact](fixtures/lob-8hp.panel-cut.json) includes exact TOML,
source/SVG digests, dimensions, roles and nominal provenance. Owner verification
recomputes all fields from source before the consumer prepares a project. The
consumer's independent verification reconstructs Transmog/Swarf operations from
the exact bound SVG and captured cutting table.

## Manual and agent runbook

Run from Reversal, with the documented sibling owner checkouts available:
The cut-only owner interface is committed at legion-of-bom
`1839620a733b545078191e8719cf3d3c27bbba48`.

```sh
cargo build --locked --manifest-path ../fpl/legion-of-bom/Cargo.toml -p legion-of-bom-panel-cli --bin lob-panel-cut
cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam
mkdir -p /tmp/lob-openwam
../fpl/legion-of-bom/target/debug/lob-panel-cut export ../fpl/legion-of-bom/examples/eurorack_8hp.toml > /tmp/lob-openwam/panel.json
../fpl/legion-of-bom/target/debug/lob-panel-cut verify /tmp/lob-openwam/panel.json
crates/sketch-intake/target/debug/openwam capabilities > /tmp/lob-openwam/capabilities.json
jq -n --slurpfile cap /tmp/lob-openwam/capabilities.json --slurpfile panel /tmp/lob-openwam/panel.json '{schema:$cap[0].request_version,request:{operation:"prepare",source_format:"svg",source_text:$panel[0].svg,settings:{model:"pro",material:"Aluminum",units:"metric",thickness:$panel[0].thickness_mm,tolerance_mm:0.02,placement_xy_down_mm:[0,0]}}}' > /tmp/lob-openwam/prepare.json
crates/sketch-intake/target/debug/openwam request /tmp/lob-openwam/prepare.json > /tmp/lob-openwam/prepared.json
jq '.result.project' /tmp/lob-openwam/prepared.json > /tmp/lob-openwam/project.json
jq -n --slurpfile cap /tmp/lob-openwam/capabilities.json --slurpfile project /tmp/lob-openwam/project.json '{schema:$cap[0].request_version,request:{operation:"verify",project:$project[0]}}' > /tmp/lob-openwam/verify.json
crates/sketch-intake/target/debug/openwam request /tmp/lob-openwam/verify.json
jq -n --slurpfile cap /tmp/lob-openwam/capabilities.json --slurpfile project /tmp/lob-openwam/project.json '{schema:$cap[0].request_version,request:{operation:"simulate",project:$project[0],fault:{kind:"none",at_ms:0}}}' > /tmp/lob-openwam/simulate.json
crates/sketch-intake/target/debug/openwam request /tmp/lob-openwam/simulate.json > /tmp/lob-openwam/replay.json
jq -n --slurpfile cap /tmp/lob-openwam/capabilities.json --slurpfile project /tmp/lob-openwam/project.json --slurpfile replay /tmp/lob-openwam/replay.json '{schema:$cap[0].request_version,request:{operation:"verify_simulation",project:$project[0],replay:$replay[0].result.replay}}' > /tmp/lob-openwam/verify-replay.json
crates/sketch-intake/target/debug/openwam request /tmp/lob-openwam/verify-replay.json
```

Check each exit code before the next command. Retain the owner artifact,
preparation request, project and replay together. This chooses Pro/Aluminum as an
explicit draft experiment; the source spec does not identify the actual material.
Using the JSON-bound SVG preserves its digest exactly; the optional owner `svg`
command adds a trailing newline when writing stdout.

For fault replay, change `fault` to `{"kind":"enclosure_open","at_ms":10000}` and
verify that replay independently using the same project. The
[execution receipt](lob-panel-demonstration.json) records both scenarios and
artifact hashes. The actual CLI completed 234/234 nominal operations in 188,264
virtual ms; the injected scenario faulted at 10,000 ms with 37 operations complete.
Both replay verifications passed.

## Scope and validation

The owner rejects unknown cutout names (including misleading footprint keywords),
unsupported source fields, invalid dimensions/placements and forged artifacts.
Connected capsules preserve slot intent without a general-purpose DXF joiner.
The reusable owner SVG renderer preserves already-resolved rounded rectangles
and rotation; the current TOML CLI supports exact nominal built-in circles only.

All 43 Reversal sketch-intake tests and four owner CLI/library tests pass. The new
Reversal regression checks dimensions, six closed IDs, six pierces, source binding,
nominal completion and independently verified fault replay. Reversal strict Clippy
passes. Owner strict Clippy finds five existing warnings in board/crosscheck;
validation with only those two lint classes allowed passes, tracked as
`legion-of-bom-8k7`. Both type checks, formatting and diff checks pass.

These artifacts verify consistency, not physical fit or a finished machining
strategy. MPN/citations and material are unresolved, explicitly unverified; the
artifact must remain beside the OpenWAM project because v1 does not embed owner
provenance. General joining/containment, cross-revision IDs, verified mechanical
resolution and the per-group project-intent API remain tracked in
`reversal-icd`, `reversal-l3u` and `reversal-ba0`. Kerf, leads, tabs and qualified
controller output remain `reversal-uhn`. No hardware I/O or machine output.
