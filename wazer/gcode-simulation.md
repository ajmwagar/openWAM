# Exported command replay

`simulate_cam_gcode` verifies a retained research candidate against its original
CAM artifact, regenerates the export, parses the actual G-code text through
Swarf, and executes the typed commands in the shared synthetic controller/plant.
It retains every emitted dwell, separate jet/abrasive gates and physical pressure
observations. `verify_cam_gcode_simulation` independently repeats the entire
source → plan → export → parse → replay sequence and compares all retained fields.

The replay is `reversal.openwam-cam-gcode-simulation.v1`. Its `cam_sha256` binds
source, material tables and full intent; `gcode_sha256` binds the exact regenerated
export text. `profile` names the explicit postprocessor. Each trace frame retains
the parsed `command`, an `observation` and separate requested/effective abrasive
and cutting-motion `channels`. `observation.operation` is the zero-based parsed
command index. This operation currently accepts the CAM export's
`historical_wam16` profile.

## Runbook

First follow [the CAM loop](cam-loop.md) to retain the panel's verified plan and
research export. From the repository, using its built CLI:

```sh
jq -n --slurpfile planned wazer/private/independent-cam-1/plan.response.json \
  --slurpfile exported wazer/private/independent-cam-1/export.response.json \
  '{schema:"reversal.openwam-request.v1",request:{operation:"simulate_cam_gcode",artifact:$planned[0].result.artifact,candidate:$exported[0].result.candidate,fault:{kind:"none",at_ms:0}}}' \
  > wazer/private/independent-cam-1/gcode-sim.request.json
target/debug/openwam request wazer/private/independent-cam-1/gcode-sim.request.json > wazer/private/independent-cam-1/gcode-sim.response.json
jq -n --slurpfile request wazer/private/independent-cam-1/gcode-sim.request.json \
  --slurpfile response wazer/private/independent-cam-1/gcode-sim.response.json \
  '$request[0] | .request.operation="verify_cam_gcode_simulation" | del(.request.fault) | .request.replay=$response[0].result.replay' \
  > wazer/private/independent-cam-1/verify-gcode-sim.request.json
target/debug/openwam request wazer/private/independent-cam-1/verify-gcode-sim.request.json > wazer/private/independent-cam-1/verify-gcode-sim.response.json
```

Set the simulation request's fault to `{"kind":"enclosure_open","at_ms":10000}`
to interrupt a cut. Repeat verification with that returned replay. Other
supported faults are independent watchdog stall, stale inputs, blocked release
and false pressure feedback. Unreachable fault times fail explicitly.
Transport success means the experiment ran; inspect `trace.outcome` for
`completed`, `faulted`, `waiting_for_release` or `unsafe_plant`.

## Timing and scope

Unlike CAM-operation replay, command replay executes all three one-second
shutdown dwells per span. The abrasive-off dwell holds jet on; the jet-off dwell
observes pressure release. No extra release wait is inserted. Trapped pressure
prevents the next rapid or successful end even after an explicit dwell expires.
The output gates only restrict controller requests; independent watchdog and
interlock cutoff remain authoritative.

Tests derive elapsed time from every parsed travel/cut feed and dwell, check
stationary dwell positions, channel sequencing, release before rapids and end,
all five faults, a host stall before the first command, and a fault during the
jet-only shutdown dwell. Verification rejects forged channels, dwell totals,
export text, digests and authority flags, and changed source intent.

The [retained CLI receipt](gcode-simulation-demonstration.json) records completion
of 155/155 commands in 383,650 virtual ms, including 54,000 ms of dwells and
578.3937529722191 mm of cutting motion. This is 17,400 ms longer than the
CAM-operation replay: 18,000 ms of emitted shutdown dwells replace six synthetic
100 ms release waits. An enclosure fault interrupts at exactly 10,000 ms after
14 completed commands. Both traces independently verify. The receipt includes
hashes for retained local requests/responses; 102 workspace tests, strict Clippy
and formatting checks passed.

This is software evidence using nominal straight-line kinematics and a synthetic
pressure model. Metadata/initialization has zero synthetic duration; rapid speed
uses the verified CAM maximum feed. Acceleration, step generation, actual WAZER
firmware behavior and hydraulic calibration are unmodeled. All machine-output,
controller and hardware qualification flags remain false; no machine I/O exists.
