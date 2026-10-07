# Pro 2.32 shutdown trace

The pinned firmware has distinct normal completion, operating-mode stop, and
pause/abort paths. Normal completion switches abrasive off first, waits on a
timer, then switches off five more output groups. Door-open handling reaches
a separate pause path with an alternate timer threshold. Neither traced handler
checks a pressure measurement before advancing to its completed/paused state.

The [machine-readable evidence](shutdown-research.json) pins the 416,324-byte
official firmware by SHA-256, function addresses, state labels, candidate output
identities and local capture hashes. This is static analysis of the downloaded
Pro 2.32 image; the installed firmware and physical board remain unidentified.
Vendor decompilation is retained privately as evidence, separate from first-party
Rust implementation. Firmware was neither executed nor changed.

## Command boundary

The handler at `0x209a0` distinguishes operating modes from job states:

| Command | Observed waterjet-module behavior |
|---|---|
| M3 | Requests `WATER_ONLY`, unless already in a pierce/water-only mode |
| M8 | Requests `PIERCE` |
| M9 | Pierce modes request `WATER_ONLY`; water-only returns; other modes request `JOGGING_OR_PAUSED` |
| M5 | Requests `JOGGING_OR_PAUSED` |
| M1403 | Sets the active bit at object offset `0x182`; this branch has no direct GPIO write |
| M1404 | Sets an end flag and requests `COMPLETED_CUT` |

Mode changes go through `0x1f490` and `0x1f0bc`. Applying a mode has state,
motion-drain, feed-hold and timer guards. In the traced module, M9 followed by M5
can turn abrasive off while preserving water-only mode, then turn the HP valve
off when leaving water-only. That does not turn every pump/peripheral off.
The subsequent [Switch/HALT trace](switch-halt-research.md) establishes that the
generic Switch matchers reject these waterjet commands. Other subscribers and
the installed wiring remain outside this trace's electrical qualification.

## Normal completion and pause

The state dispatcher at `0x23520` calls normal completion `0x20264` for state
`0x64`. In source order, its enabled-pin branches request:

1. Abrasive valve logical zero.
2. Start/revisit timer at object `+0x124`, comparing against `+0xb0`.
3. After that threshold: low-pressure pump, abrasive motor, HP valve, HP pump,
   and dump valve logical zero, in that order.
4. Clear bookkeeping and enter `ALL_DONE` (`0x65`).

The door handler `0x21058` sets pause reason `2`, then calls prepare-pause
`0x20f94`. This enters `PREPARE_TO_PAUSE` (`0x68`), requests feed hold, cancels
mode timers, switches abrasive off and enters `PAUSING` (`0x69`). Handler
`0x20650` selects threshold `+0xb4` for reason `2`, otherwise `+0xb0`. After
that timer it switches the same five remaining groups off and enters `PAUSED`
(`0x6a`). Water-level and other faults also feed this shared path with their own
reason codes.

Constructor `0x1d1f4` initializes `+0xb0` with float bits `0x3f800000` (1.0)
and `+0xb4` with `0x3dcccccd` (approximately 0.1). These are static timer
defaults, not measurements of hydraulic release. Runtime aliases/overrides and
actual board response have not been exhaustively resolved.

The embedded `release_pressure_seconds = 0.4` belongs to a different field.
Config loading at `0x23f7c` matches checksum `0x7911` and stores it at `+0xd8`.
Helper `0x1cd98` waits on that field, enables the HP valve, waits again, then
disables it. The 0.4-second text therefore cannot be used as the normal completion
delay or as proof of pressure release.

Candidate output names come from config-key checksum matching, corroborated by
the retained [upstream checksum implementation](https://github.com/Smoothieware/Smoothieware/blob/38e2cc083db0e4f768535a9bf2d32cdf104ea980/src/libs/utils.cpp).
The vendor Pin parser at `0x73a4` stores inversion for `!`, and writer `0x1d16c`
chooses the GPIO set/clear register according to logical value and inversion.
“Logical zero” consequently does not establish connector voltage, valve position
or clutch/relief behavior on the installed Pro revision.

## Abort and halt scope

The end-stop/abort state handler `0x22ef4` stops motion through `0xcd1c`, invokes
player-abort helper `0x22e7c`, then calls prepare-pause. The helper dispatches a
public-data request using identifiers `0xef8f/0xba31`, changes flags and releases
feed hold as applicable. Halt callback `0x22fa0` conditionally invokes that helper
and requests the end-stop state for several current states.

This establishes a software route into the pause machinery. It does not establish
every global HALT subscriber, Switch fail-safe behavior, watchdog breakpoint
shutdown or power-loss output state. The subsequent [Switch/HALT trace](switch-halt-research.md)
resolves Switch command exclusions, HALT defaults and relative registration order.
The complete vendor debugger path and power-loss behavior still require evidence.

## Effect on OpenWAM

Our [exported-command simulator](gcode-simulation.md) correctly retains emitted
M9/M5 ordering and all dwells as a synthetic preview. Its jet/abrasive gates omit
the firmware's additional pump/motor/dump outputs, internal mode timers and
M1404 completion handler. Its independent cutoff and pressure-release feedback
remain explicit synthetic assumptions. Keep this model identified separately
from a future firmware sequence model; do not silently replace its constants
with the constructor or config values above.

The [bounded Rust replay](shutdown-replay.md) now covers normal completion,
door pause and explicitly supplied abort events, with six logical output groups
and synthetic output faults. [G-code integration](gcode-shutdown.md) derives
completion from retained command replay. The [shared timer trace](shared-shutdown-timer.md)
pins mode application as a third owner of the completion/pause timer; Player
callbacks, full state guards, runtime config precedence and debugger execution
remain outside the delivered replay. No machine/controller/hardware qualification
follows from matching static paths.

## Reproduce the evidence

Retained files are ignored under `wazer/private/shutdown-v232/`. Original bytes
were copied without changes from the Reversal acquisition. Reopen its existing
Ghidra project at
`/Users/ajmwagar/Documents/Reversal/Ghidra/wazer-pro-v232-20261006.gpr`, selecting
`/firmware-v2.32-base4000.bin`. Confirm base `0x4000`; the old zero-base import is
superseded. In the headless bridge, load the project program using the `path`
argument and include the program name in every subsequent query.

```sh
shasum -a 256 wazer/private/shutdown-v232/firmware-v2.32.bin
arm-none-eabi-objdump -D -b binary -m arm -M force-thumb --adjust-vma=0x4000 \
  --start-address=0x20264 --stop-address=0x203e8 \
  wazer/private/shutdown-v232/firmware-v2.32.bin
```

For each retained `shutdown-bytes-ADDRESS.json`, subtract `0x4000` from its
mapped address, read exactly 64 bytes from the original and compare its hex
with the bridge's `hex` field. All 20 function spans matched. State-label pointer
and constructor-constant captures are also compared against original bytes.
Decompilation and raw disassembly must agree on branches/writes; a decompiler
label alone does not establish state meaning. Capture hashes detect retained
file drift; they do not authenticate a source or prove runtime behavior.
