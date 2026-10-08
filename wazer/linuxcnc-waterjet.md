# Verified waterjet CAM to LinuxCNC research G-code

OpenWAM's `export_cam_linuxcnc` API calls Swarf's owner postprocessor after
rebuilding the original SVG/DXF, material-table selection and cut intent. The
export retains a CAM digest, target profile, G-code digest, span and cut-move
counts, and planner limitations. `verify_cam_linuxcnc` independently regenerates
all of them; modified source plans, commands, profile-only edits, digests and
qualification flags reject. Existing WAZER export and simulation APIs remain
separate.

The output uses XY millimetres, absolute G54 coordinates, cleared G92 offsets,
feed per minute and G61 exact-path mode. Compensation, tab gaps, lead geometry,
hole order and selected starts come from the CAM owner; export does not apply
another kerf offset. No Z motion, probing or work-offset setting is generated.
Coordinates and feeds are written with eight fractional digits. The exporter
rejects moves that collapse at that precision and checks both original and
rounded coordinates against explicit travel limits.

Each span runs with this sequence: outputs off during rapid; jet on; optional
jet-settle dwell; abrasive on; pierce dwell; feed cuts; abrasive off; optional
jet-on flush dwell; jet off. Initial outputs are explicitly cleared. Both are
off before M2. Dwell units are seconds. Digital outputs are explicit distinct
indices below the caller's configured output count; no spindle or coolant aliases
are guessed. LinuxCNC's [M64/M65 commands](https://linuxcnc.org/docs/stable/html/gcode/m-code.html#sec:M62-M65)
change outputs immediately and break blending, while queued M62/M63 changes need
a following motion. Immediate commands allow stationary startup and final shutdown.
The associated `motion.digital-out-nn` pins require separately configured HAL
wiring. [G4](https://linuxcnc.org/docs/stable/html/gcode/g-code.html#sec:G4-Dwell)
does not itself change process outputs.

The retained fixture profile is synthetic, including startup/flush delays and
travel limits. Its table feed and pierce values come from the existing source-pinned
WAZER table fixture; they do not establish settings for another waterjet. Machine
output and controller qualification remain false. Startup time is an open-loop
delay, not measured pressure readiness. This export does not implement HAL
interlocks, emergency shutdown, acceleration/quality compensation or physical
machine transfer. It is not a WAZER-compatible cut file.

Reproduce from the repository root (requires existing `jq`):

```sh
cargo build --locked -p reversal-sketch-intake --bin openwam
mkdir -p /tmp/openwam-linuxcnc-waterjet
target/debug/openwam capabilities > /tmp/openwam-linuxcnc-waterjet/capabilities.json
jq --rawfile source wazer/fixtures/cam-panel.svg '.prepare_example | .request.source_text=$source' /tmp/openwam-linuxcnc-waterjet/capabilities.json > /tmp/openwam-linuxcnc-waterjet/prepare.json
target/debug/openwam request /tmp/openwam-linuxcnc-waterjet/prepare.json > /tmp/openwam-linuxcnc-waterjet/project.json
jq -n --slurpfile project /tmp/openwam-linuxcnc-waterjet/project.json --slurpfile intent wazer/fixtures/cam-panel.intent.json '{schema:"reversal.openwam-request.v1",request:{operation:"plan_cam",project:$project[0].result.project,intent:$intent[0]}}' > /tmp/openwam-linuxcnc-waterjet/plan.request.json
target/debug/openwam request /tmp/openwam-linuxcnc-waterjet/plan.request.json > /tmp/openwam-linuxcnc-waterjet/cam.json
jq -n --slurpfile cam /tmp/openwam-linuxcnc-waterjet/cam.json --slurpfile profile wazer/fixtures/linuxcnc-waterjet.profile.json '{schema:"reversal.openwam-request.v1",request:{operation:"export_cam_linuxcnc",artifact:$cam[0].result.artifact,profile:$profile[0]}}' > /tmp/openwam-linuxcnc-waterjet/export.request.json
target/debug/openwam request /tmp/openwam-linuxcnc-waterjet/export.request.json > /tmp/openwam-linuxcnc-waterjet/export.json
jq -rj '.result.candidate.code.gcode' /tmp/openwam-linuxcnc-waterjet/export.json > /tmp/openwam-linuxcnc-waterjet/panel.nc
jq -n --slurpfile cam /tmp/openwam-linuxcnc-waterjet/cam.json --slurpfile code /tmp/openwam-linuxcnc-waterjet/export.json '{schema:"reversal.openwam-request.v1",request:{operation:"verify_cam_linuxcnc",artifact:$cam[0].result.artifact,candidate:$code[0].result.candidate}}' > /tmp/openwam-linuxcnc-waterjet/verify.request.json
target/debug/openwam request /tmp/openwam-linuxcnc-waterjet/verify.request.json
```

An actual CLI integration test compares every motion coordinate and feed against
the independently rebuilt compensated plan, counts separate pierced tab spans,
and rejects tampered candidates. The Swarf process-trace test verifies no powered
rapids, startup/pierce/flush output states and final shutdown. These tests establish
export semantics, not native LinuxCNC trajectory or hardware execution.

The retained panel run produced **6 pierced spans, 90 cutting moves and 4,684
G-code bytes**. Independent API regeneration verified SHA-256
`48db49eab69042d93e216b99bbe577097a4ad61ca4ea8472183832ed1d164811`.
See [the demonstration receipt](linuxcnc-waterjet-demonstration.json) and
[exact exported fixture](fixtures/linuxcnc-panel.nc). Native interpreter/HAL
validation is tracked in Reversal issue `reversal-rfc`; the existing isolated
LinuxCNC VM exited during startup, so no native execution was established.
