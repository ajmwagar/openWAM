# Offline shutdown replay

OpenWAM now has a separate Rust model of the traced Pro 2.32 shutdown subset.
It records six logical output channels, requested GPIO levels and synthetic
electrical observations. An `all_done` or `paused` software state can coexist
with a stuck or still-delayed output; `final_output_mismatches` makes that visible.
The existing CAM/G-code synthetic replay retains its own behavior.

The [retained CLI demonstration](shutdown-replay-demonstration.json) covers
completion, door pause, stuck HP-pump output and ordered HALT/abort/clear.
Every response was repeated byte-for-byte and independently recomputed; changing
one reference timestamp produced one reported difference. The stuck-output case
still reaches `all_done` while identifying the HP-pump mismatch.

Use `reversal_sketch_intake::shutdown::{simulate, verify, compare}` directly or
the agent CLI. `capabilities` derives schemas from the same types and includes
`shutdown_example`. The example's thresholds are constructor defaults and its
all-on initial outputs are a synthetic starting condition, not an installed
configuration.

```sh
cargo run --locked -p reversal-sketch-intake --bin openwam -- request \
  wazer/fixtures/shutdown-completion.request.json > /tmp/shutdown-replayed.json
jq '.result.replay | {final_state,requested_logical,final_output_mismatches,trace}' \
  /tmp/shutdown-replayed.json

jq -n --slurpfile request wazer/fixtures/shutdown-completion.request.json \
  --slurpfile response /tmp/shutdown-replayed.json \
  '{schema:$request[0].schema,request:{operation:"verify_shutdown_replay",
    input:$request[0].request.input,replay:$response[0].result.replay}}' \
  > /tmp/shutdown-verify.json
cargo run --locked -p reversal-sketch-intake --bin openwam -- request \
  /tmp/shutdown-verify.json
```

## Events and configuration

Timestamps are integer microseconds. Equal timestamps execute in input order.
Events are limited to 1,024 over 60 seconds; clock regressions, unsupported
transitions and invalid thresholds fail explicitly. At most 16 digital Switch
bindings are allowed. Six pins follow this order: low-pressure pump, abrasive
motor, high-pressure pump, high-pressure valve, abrasive valve, dump valve.
Each pin declares connection, inversion and its initial logical value.

| Event | Modeled subset |
|---|---|
| `begin_completion` | Request `completed_cut`; first following poll switches abrasive off and starts a fresh timer |
| `prepare_pause`, with reason | `prepare_to_pause`, feed hold, stop mode timers, abrasive off, then `pausing`; first poll starts timer |
| `abort`, with reason | Stop-motion/player-abort calls followed by the modeled prepare-pause sequence |
| `switch_halt`, active | Apply explicit ordered digital Switch values, respecting ignore and connection flags |
| `switch_halt`, inactive | Record clear; do not restore Switch outputs or cancel an active pause timer |
| `poll` | Run the selected completion/pause handler; unrelated states perform no work |

Completion polls repeat the abrasive-off write. When elapsed time exceeds the
selected threshold, the remaining writes occur in traced source order:
low-pressure pump, abrasive motor, high-pressure valve, high-pressure pump,
dump valve. Pause reason `2` selects the door threshold; other reasons and normal
completion use the normal threshold. No pressure input gates these transitions.

The [timer-boundary receipt](shutdown-timer-boundary.json) retains original
disassembly hashes. Timer read `0x3f6e4` converts an integer interval to f32 seconds using
divisor bits `0x49742400` (1,000,000). Comparator `0x41cf0` swaps arguments through
`0x41c88`, compares through `0x41c90`, and returns the carry-clear result. For
finite nonnegative inputs this tests **threshold < elapsed**, not equality.
The replay preserves f32 conversion and strict comparison. Exactly 1,000,000 us
at threshold 1.0 waits; a following poll at 1,000,001 us advances. Starting the
first poll later shifts the timer epoch. A poll schedule must therefore be an
explicit input, not inferred from an emitted G-code dwell.

## Faults, comparison and verification

`hold_electrical` fixes one connected channel at a supplied synthetic level.
`delay_electrical` delays each write on one channel by `delay_us`, retaining
source order for equal-time writes. Delayed changes are observed at their due
timestamps as the replay advances, even without a handler poll at that instant.
Changes due after the last input timestamp remain pending. These faults do not
alter vendor state progression and do not model hydraulic pressure or valve
position. Disconnected channels have unknown electrical observations.

| Operation | Inputs | Result |
|---|---|---|
| `replay_shutdown` | Explicit config, fault and event schedule | Evidence/input-bound trace and final output differences |
| `verify_shutdown_replay` | Independent input plus retained replay | Full recomputation; altered input, evidence, actions or flags rejected |
| `compare_shutdown_trace` | Input and caller-supplied typed reference trace | Exact timestamp/action comparison, total differences, first 32 details |

Replays bind the firmware identity owned by the shutdown receipt and all three
compiled-in research receipt digests. Verification is deterministic recomputation;
it does not authenticate a producer. A matching caller reference is an exact
trace comparison, not firmware-execution parity. Tests additionally use manually
specified output order and timer boundaries from the retained disassembly.

## Scope

The model implements the output/timer subset in [shutdown research](shutdown-research.md)
and digital Switch behavior from [Switch/HALT research](switch-halt-research.md).
It assumes a fresh timer epoch at entry and initially clear feed hold. It abstracts
player/feed-hold bookkeeping and does not model integer timer wraparound, IRQ
timing or shared-timer reuse when aborting an already-running completion handler;
completion-to-pause/abort transitions are explicitly rejected. It also excludes
operating-mode application, PWM Switches, configuration parsing or
debugger execution. Waterjet HALT has unresolved guards: supply a separate resolved
`abort` event rather than automatically inventing that transition from Switch HALT.
The [shared timer trace](shared-shutdown-timer.md) now pins mode application as
another owner of that timer and identifies state overrides and Player callbacks
that must be resolved before extending these transitions.
The trace is independently authored Rust based on observations; private vendor
decompilation is not linked into it.

The [G-code integration](gcode-shutdown.md) now derives completion dispatch from
the verified exported-command replay, retaining emitted dwell time separately
from internal handler time.
Mode guards and the complete HALT/debugger path remain research prerequisites for
a broader firmware behavior model, now tracked under `reversal-3ym.1`.
