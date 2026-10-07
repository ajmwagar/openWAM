# Libre waterjet control policy

An independently written, GPL-3.0-or-later Rust `no_std` library with no
dependencies or allocations. It is a simulation foundation for a future WAZER
Pro port. It does not contain firmware startup, pin/register access, a motion
planner, a G-code interpreter, SD updates, or a machine driver.

`Controller::step(now_ms, inputs, command)` is the entire control boundary.
`state()` and `requests()` return the resulting state and abstract jet,
abrasive, and cutting-motion requests. Inputs use logical permissions with
explicit unknown values, independently timestamped samples, and pressure-release
feedback. A caller must use a monotonic clock and supply a nonzero input-age
budget; the example's 50 ms is synthetic, not a verified machine requirement.

Sequence: disarmed → arm → armed → pierce → piercing → cut → cutting → stop →
depressurizing → confirmed pressure release → disarmed. Any invalid transition,
unknown/failed permission, stale input, time regression, future sample, or abort
latches a fault and removes all requests. Reset requires fresh, clear inputs and
confirmed pressure release, and returns only to disarmed. No automatic resume.

Pressure-release false is expected during a cut. It is not a permission to arm
or reset. A fault records its first cause until a successful reset. A stopped CPU
cannot execute this policy: an independently verified watchdog and electrical
interlocks remain hardware requirements. No claim is made that switching all
requests off implements the WAZER's correct hydraulic shutdown sequence.

Pierce completion and cut commands are supplied by a future sequence executor;
this core does not invent pierce delays, abrasive preflow, valve ordering,
pressure thresholds, acceleration, or motion completion. All are outstanding
qualification work. The proposed logical inputs are requirements to investigate,
not a claim that corresponding physical sensors exist on this machine.

Run the manual verification loop from the Reversal root:

```sh
cargo check --manifest-path crates/waterjet-control/Cargo.toml --all-targets
cargo test --manifest-path crates/waterjet-control/Cargo.toml
cargo clippy --manifest-path crates/waterjet-control/Cargo.toml --all-targets -- -D warnings
cargo run --manifest-path crates/waterjet-control/Cargo.toml --example simulate
```

The example uses synthetic inputs: arm, pierce, cut, open enclosure, refused
reset with pressure present, successful reset after release. It writes only to
stdout. Tests exhaust all 7^5 short command sequences for two pressure feedback
values (33,614 sequences), plus specific fault and recovery scenarios. This is
bounded testing, not a proof for all possible sequences or physical behavior.

[Port plan](../../wazer/libre-firmware.md) records owner boundaries and the
evidence needed before a hardware implementation. Neither proprietary vendor
source nor decompiler output is incorporated in this library. GPL licensing is
chosen to permit a deliberate future integration with GPLv3 Smoothieware; no
upstream executable code is included. `COPYING` contains the GPLv3 license text.

## Virtual-time plant simulator

The optional `std` feature adds a host simulator; the default core remains
`no_std`, allocation-free and dependency-free. Run all nine fixed scenarios:

```sh
cargo run --manifest-path crates/waterjet-control/Cargo.toml --features std --example plant
cargo run --manifest-path crates/waterjet-control/Cargo.toml --features std --example plant -- --scenario watchdog-stall
cargo test --manifest-path crates/waterjet-control/Cargo.toml --features std
```

The plant advances in bounded 1 ms virtual steps. Its independent supervisor can
remove effective requests while the core is stalled; restarting the host aborts
the core and never clears the supervisor's latch. Requested outputs, effective
outputs, physical pressure, nozzle flow and sensor observations are distinct.
Plant pressure uses arbitrary integer units, rise/decay and an unverified supply
cutoff. Default 50 ms input age and 100 ms watchdog are synthetic experiment
parameters. They are unrelated to the vendor's embedded 10-second watchdog
configuration. No actual wiring, pump/clutch/valve sequence or pressure sensor is
implemented, and hydraulic dynamics are not a calibrated physics model.

Scenarios: normal shutdown, enclosure opening, host stall/watchdog trip, stale
inputs, unknown water input, trapped pressure, a stuck-open nozzle valve, a failed
cutoff and false pressure-release feedback. A successful run means the script's
expected result was verified. `HazardDemonstrated` explicitly marks residual flow,
failed cutoff or premature recovery; it does not mean the fault was contained.
The pressure-sensor case exposes a limitation of the current core: it trusts a
single release observation. The simulator detects the discrepancy using plant
truth that a real controller would not automatically possess.

Output is deterministic TSV, ready for retained traces and comparison. All
requested scenarios are computed and checked before report output. Unknown names
or unexpected outcomes fail with nonzero exit status. `Simulator` also accepts
custom synthetic `Model` and `Injection` values for host tests. Invalid clocks,
oversized advances and invalid model parameters fail explicitly. Six additional
plant tests cover watchdog boundary, residual pressure, independent interlock,
replay, chunk invariance and demonstrated unsafe outcomes.

[Board/hydraulic/watchdog research](../../wazer/board-contracts.md) records the
initial binary and manufacturer evidence without promoting it to qualification.
