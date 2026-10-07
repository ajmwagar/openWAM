# Verified geometry to simulated waterjet job

The job replay calls existing owners: original SVG/DXF → Transmog geometry →
Swarf waterjet draft → Rust control core and virtual-time plant. It verifies the
integration against original bytes and captured material tables before execution.
No deserialized operation list is accepted as authority.

Create a draft using the [integration runbook](integration.md), then:

```sh
cargo run --manifest-path crates/sketch-intake/Cargo.toml -- --simulate-integration crates/sketch-intake/examples/plate.svg wazer/private/pro-reference/plate-integration.json none > wazer/private/pro-reference/plate-simulation.json
cargo run --manifest-path crates/sketch-intake/Cargo.toml -- --verify-simulation crates/sketch-intake/examples/plate.svg wazer/private/pro-reference/plate-integration.json wazer/private/pro-reference/plate-simulation.json
cargo run --manifest-path crates/sketch-intake/Cargo.toml -- --simulate-integration crates/sketch-intake/examples/plate.svg wazer/private/pro-reference/plate-integration.json enclosure_open 10000 > wazer/private/pro-reference/plate-fault-simulation.json
```

Reports bind original, integration and table hashes, model parameters, explicit
fault scheduling, partial positions and completed operation counts. The verifier
rebuilds the entire report from the originals rather than trusting reported
completion, positions or eligibility.

Rapids and cuts are a straight-line kinematic preview at the nominal feed/gantry
limit. Pierce time comes from the selected public table row. Each duration rounds
up to milliseconds; acceleration, step generation, kerf, leads, tabs and a
qualified postprocessor remain absent. Polling and the independent watchdog
advance in virtual time. Fault boundaries split movement; a stalled host does
not renew the watchdog, and no cut resumes automatically.

Stop waits for modeled pressure feedback. Exhausting the synthetic release wait
budget yields `waiting_for_release`, never successful release. False release
feedback can produce `unsafe_plant`; the report retains the observation before
abort. Neither case proceeds to the next contour.

Fault names: `none`, `enclosure_open`, `watchdog_stall`, `stale_input`,
`release_blocked`, `pressure_sensor_lies`. Faults require an explicit virtual
millisecond timestamp. A scheduled fault after the completed job fails as
unreached. Replays are bounded to 10,000 operations and one hour of virtual time;
verification accepts reports up to 16 MiB.

This is an offline simulation. All model constants are synthetic, all machine
output and hardware qualification fields remain false, and the installed WAZER
board/hydraulic contracts still require [evidence](board-contracts.md).

The controller owner is GPL-3.0-or-later. The combined simulator executable
includes that GPL component; existing intake and geometry modules retain their
own source licenses.

The retained [2026-10-06 execution receipt](job-simulation-acquisition.json)
records all 125 plate operations completing in 207,158 virtual milliseconds,
with 336.209 mm of simulated cut travel and a final disarmed, zero-pressure
model. Opening the enclosure at 10,000 ms interrupts operation 2 at that exact
boundary, drops effective jet output and preserves the partial position. Pressure
is still 1,000 synthetic units at that fault boundary; output-off is not evidence
of released pressure. Both retained reports passed independent recomputation.
The intake suite has 30 passing tests and the controller suite has 12.
