# Verified CAM simulation and research export

The independent CAM artifact now closes a deterministic software loop:
SVG/DXF → compensated cut spans → fault-tested replay → research G-code →
independent verification. The simulator and Swarf postprocessor consume the
same verified CAM operations, including holes-first ordering, waste-side leads,
uncut tab gaps and a separate pierce for every span after a tab.

## Bound artifacts

Four agent operations extend the existing JSON API:

| Operation | Inputs | Output |
|---|---|---|
| `simulate_cam` | `artifact`, `fault` | `result.replay`, schema `reversal.openwam-cam-simulation.v1` |
| `verify_cam_simulation` | `artifact`, retained `replay` | Independent verification receipt |
| `research_cam_gcode` | `artifact`, explicit `profile` | `result.candidate`, schema `reversal.openwam-cam-research-candidate.v1` |
| `verify_cam_gcode` | `artifact`, retained `candidate` | Independent regeneration receipt |

`cam_sha256` hashes the independently rebuilt canonical CAM artifact, including
source/project, tables, full contour intent and derived operations. Verification
rebuilds the inputs and compares all retained output fields. Changing valid intent
invalidates earlier evidence even if the toolpath happens to remain identical.
Unknown fields, forged frames/code/digests and enabled authority flags fail.

The replay wrapper retains the underlying shared synthetic replay as its `replay`
field. Within that trace, the legacy field `integration_sha256` carries the CAM
artifact digest; the outer `cam_sha256` names this binding explicitly. Synthetic
motion uses nominal table feeds and rapid limits. Additional spans include their
own pierce and observed synthetic pressure-release wait before the next rapid.

The export currently requires `historical_wam16`. It invokes Swarf's existing
multi-path research postprocessor, preserving every compensated cut and travel
coordinate. It does not reuse WAM's SVG importer. `wam24_pro_presegmented` is
explicitly rejected: that separate service profile requires one closed supplied
path and is not qualified for tabbed, open, separately pierced CAM spans.

## Manual and agent runbook

First prepare/plan the panel using [the independent CAM runbook](independent-cam.md).
Then run from the OpenWAM repository:

```sh
jq -n --slurpfile planned wazer/private/independent-cam-1/plan.response.json \
  '{schema:"reversal.openwam-request.v1",request:{operation:"simulate_cam",artifact:$planned[0].result.artifact,fault:{kind:"none",at_ms:0}}}' \
  > wazer/private/independent-cam-1/simulate.request.json
target/debug/openwam request wazer/private/independent-cam-1/simulate.request.json > wazer/private/independent-cam-1/simulate.response.json
jq -n --slurpfile planned wazer/private/independent-cam-1/plan.response.json \
  --slurpfile replay wazer/private/independent-cam-1/simulate.response.json \
  '{schema:"reversal.openwam-request.v1",request:{operation:"verify_cam_simulation",artifact:$planned[0].result.artifact,replay:$replay[0].result.replay}}' \
  > wazer/private/independent-cam-1/verify-simulation.request.json
target/debug/openwam request wazer/private/independent-cam-1/verify-simulation.request.json > wazer/private/independent-cam-1/verify-simulation.response.json
jq '.request.operation="research_cam_gcode" | .request.profile="historical_wam16" | del(.request.fault)' \
  wazer/private/independent-cam-1/simulate.request.json > wazer/private/independent-cam-1/export.request.json
target/debug/openwam request wazer/private/independent-cam-1/export.request.json > wazer/private/independent-cam-1/export.response.json
jq -r '.result.candidate.code.gcode' wazer/private/independent-cam-1/export.response.json > wazer/private/independent-cam-1/panel.research.gcode
jq -n --slurpfile planned wazer/private/independent-cam-1/plan.response.json \
  --slurpfile exported wazer/private/independent-cam-1/export.response.json \
  '{schema:"reversal.openwam-request.v1",request:{operation:"verify_cam_gcode",artifact:$planned[0].result.artifact,candidate:$exported[0].result.candidate}}' \
  > wazer/private/independent-cam-1/verify-export.request.json
target/debug/openwam request wazer/private/independent-cam-1/verify-export.request.json > wazer/private/independent-cam-1/verify-export.response.json
```

For fault replay, set `fault` to `{"kind":"enclosure_open","at_ms":10000}`
in the simulation request and independently verify its returned replay. Transport
success means the experiment ran, not that the job completed. The retained outcome
distinguishes a fault, waiting for pressure release and an unsafe synthetic plant.

## Evidence and limits

CLI regression tests replay the four-path, six-span panel through all 108
operations, check every cut/travel endpoint, verify that rapids occur with jet
and nozzle flow off, and parse the exported G-code to check every move/feed and
all six jet-on/off sequences. The length check subtracts both 3 mm tab gaps from
compensated perimeters and adds 22 mm of requested leads. Fault tests exercise
the enclosure, independent watchdog, stale inputs, blocked release and false
release feedback. Mutation tests reject changed intent, frames, code and authority.

The [actual CLI receipt](cam-loop-demonstration.json) records a nominal completed
replay of 108/108 operations in 366,250 virtual ms with 578.3937529722191 mm of
cutting motion. The historical export has six paths, 90 linear moves, six rapids,
155 commands and no unresolved feeds. An enclosure fault at 10,000 ms interrupts
after three completed operations. Nominal replay, fault replay and export all
independently verify; input/output hashes reference retained local files.

This establishes software consistency, not manufacturer/controller compatibility.
The `simulate_cam` simulator executes CAM operations; historical export adds
three one-second shutdown dwells per span. The separate
[exported command replay](gcode-simulation.md) now executes these exact dwells
with separate jet/abrasive channels. CAM replay observes its own release dynamics.
Neither model calibrates
actual WAZER hydraulics, corner feeds, acceleration, tab strength or hardware.
Source/profile qualification warnings remain in the owner export alongside CAM
geometry limits. Every machine/controller/hardware qualification flag stays false.
