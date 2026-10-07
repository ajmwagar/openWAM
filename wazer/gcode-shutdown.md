# Exported G-code and completion replay

`simulate_cam_gcode_shutdown` connects a verified CAM export to the offline
shutdown subset. It regenerates and parses G-code, runs the existing synthetic
command replay, and derives `begin_completion` only from a successfully completed
M1404 (`End`) command. The original command trace is retained unchanged.

The completion trace starts a separate relative clock at that dispatch point.
`completion.config_at_dispatch` explicitly supplies the six-channel snapshot,
inversion/connection settings and timer thresholds. `completion.output_fault`
selects an optional synthetic stuck/delayed output. `completion.poll_offsets_us`
supplies handler polls relative to dispatch. None of these settings are inferred
from M3/M5 or M8/M9, whose full mode guards remain outside this integration.

## Run and verify

The [retained CLI receipt](gcode-shutdown-demonstration.json) covers nominal,
interrupted and stuck-output cases. Responses repeated byte-for-byte and all
verified; the nominal inner command trace matches the earlier retained replay.

Follow [the CAM loop](cam-loop.md) to produce the panel's plan/export files, then
build the CLI. The [shutdown example](fixtures/shutdown-completion.request.json)
supplies explicitly synthetic all-on outputs and constructor-default thresholds.

```sh
jq -n --slurpfile planned wazer/private/independent-cam-1/plan.response.json \
  --slurpfile exported wazer/private/independent-cam-1/export.response.json \
  --slurpfile example wazer/fixtures/shutdown-completion.request.json \
  '{schema:"reversal.openwam-request.v1",request:{
    operation:"simulate_cam_gcode_shutdown",artifact:$planned[0].result.artifact,
    candidate:$exported[0].result.candidate,fault:{kind:"none",at_ms:0},
    completion:{config_at_dispatch:$example[0].request.input.config,
      output_fault:{kind:"none"},poll_offsets_us:[0,1000000,1000001]}}}' \
  > /tmp/gcode-shutdown.request.json
target/debug/openwam request /tmp/gcode-shutdown.request.json \
  > /tmp/gcode-shutdown.response.json

jq -n --slurpfile request /tmp/gcode-shutdown.request.json \
  --slurpfile response /tmp/gcode-shutdown.response.json \
  '$request[0] | .request.operation="verify_cam_gcode_shutdown" |
    .request.replay=$response[0].result.replay' > /tmp/gcode-shutdown.verify.json
target/debug/openwam request /tmp/gcode-shutdown.verify.json
```

Verification receives the original artifact, candidate, command fault and
completion settings independently of the retained result. It recomputes the
full source/intent/table/export/command/completion sequence. Changed dispatch
times, settings, nested traces or qualification flags fail comparison.
Simulation and verification share a 16 MiB replay budget. There can be at most
1,023 completion polls within the standalone model's 60-second relative horizon.
Unsorted schedules fail even if the command program is interrupted.

## Read the two clocks

`command_replay.trace.virtual_elapsed_ms` and `dwell_elapsed_ms` retain emitted
command timing. `completion.dispatch_at_program_us` is the synthetic command
dispatch timestamp, converted from millisecond resolution. The internal handler
trace uses relative microseconds. Its first poll starts the timer; equality with
the threshold still waits. `last_observation_at_program_us` maps the final
relative poll back to the program clock without altering any emitted dwell.

With the supplied panel and example schedule:

| Quantity | Result |
|---|---|
| Exported commands | 155 completed |
| Emitted dwells | 54,000 ms |
| M1404 dispatch | 383,650,000 program µs, index 154 |
| Equality poll | Relative 1,000,000 µs; still waiting |
| Completion poll | Relative 1,000,001 µs; `all_done` |
| Last combined observation | 384,650,001 program µs |

`completion_status: "dispatched"` means the command reached this boundary; it
does not mean the supplied poll schedule finished shutdown. An equality-only
schedule leaves `relative_replay.final_state` at `completed_cut`. A stuck pump
can coexist with `all_done` and a reported output mismatch.

If the synthetic program faults before End, or End is rejected while waiting
for release, status is `not_dispatched` and `completion` is null. No vendor HALT,
door-pause or abort sequence is fabricated from a synthetic fault. Those resolved
events can be exercised separately through [shutdown replay](shutdown-replay.md).

This closes the exported-command/completion integration milestone. Full operating
mode trajectories, runtime config precedence, shared timer reuse and vendor
HALT/debugger guard behavior remain separately tracked firmware research.
That remaining scope is `reversal-3ym.1`; the completed integration is `reversal-faq.3`.
