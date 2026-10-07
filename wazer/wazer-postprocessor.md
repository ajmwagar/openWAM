# Observed WAZER dialect and Rust research postprocessor

The captured WAM client contains G-code generation. Static inspection of the
hash-pinned `main-SLLISYKG.js` resolves 6,891 string entries at rotation 431
without executing JavaScript. The client declares **2.4.0**, absolute mm output,
negative-down Y and two-decimal formatting. Its serializer contains `G90/G21`,
`M1403–M1413`, `G0/G1`, `M3/M8`, dwell and `M9/M5` sequences. It also contains
corner slowdown, tabs and small-path piercing; those features are not reproduced.

The independent implementation lives in Swarf's `swarf_waterjet::wazer` module.
Reversal calls this owner through OpenWAM's typed API. No proprietary JS source
or executable bundle is incorporated into the implementation. Captured assets
and manufacturer exports remain in ignored private storage; tracked receipts
contain hashes, observations and summaries.

## Scope and profiles

The explicit export profile `historical_wam16` follows the two official welcome
exports tagged WAM **1.6**. Current client 2.4.0 compatibility remains unqualified.
The postprocessor revalidates a Swarf draft, produces a typed `Program`, checks
abstract motion/output ordering, serializes CRLF text and reparses it to require
identical command semantics.

The parser supports a bounded observed subset. Unknown commands, duplicate motion
words, unsupported axes/units, nonfinite numbers, injected metadata, moves outside
declared extents and incomplete shutdown fail. Modal feed is retained; moves
before any explicit feed are reported as unresolved. No feed default is invented.
Limits are 16 MiB G-code, 1,024 bytes per line and 400,000 commands.

Candidates preserve source contour order and centrelines, with explicit feed on
every cutting move. They use the selected material's pierce time and three observed
one-second stop dwells. Header extents use the full declared cutting area; material
labels use catalog names. Numeric formatting preserves f64 round trips. These
policies remain unqualified for controller interpretation. Display time includes
nominal cutting, piercing and these dwells, excluding rapid, acceleration and
hydraulic behavior.

`machine_output_enabled` and `controller_qualified` remain **false**. Verification
means reproducible research output. Installed firmware remains unknown. The
historical G-code is not a geometry oracle for the retained DXFs: matching geometry
has not been established. Kerf, leads, tabs, starts, ordering, corner behavior and
physical timing remain broader `reversal-uhn` qualification work. No hardware
transfer, execution or flashing was performed.

## Agent and manual runbook

Build the ordinary CLI and discover its generated schemas:

```sh
cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam
crates/sketch-intake/target/debug/openwam capabilities
```

Starting from a verified saved project:

```sh
jq -n --slurpfile project /tmp/panel.openwam.json '{schema:"reversal.openwam-request.v1",request:{operation:"research_gcode",project:$project[0],profile:"historical_wam16"}}' > /tmp/post-request.json
crates/sketch-intake/target/debug/openwam request /tmp/post-request.json > /tmp/post-response.json
jq -n --slurpfile project /tmp/panel.openwam.json --slurpfile posted /tmp/post-response.json '{schema:"reversal.openwam-request.v1",request:{operation:"verify_research_gcode",project:$project[0],candidate:$posted[0].result.candidate}}' > /tmp/verify-post.json
crates/sketch-intake/target/debug/openwam request /tmp/verify-post.json
jq -n --slurpfile posted /tmp/post-response.json '{schema:"reversal.openwam-request.v1",request:{operation:"inspect_gcode",gcode_text:$posted[0].result.candidate.code.gcode}}' > /tmp/inspect-post.json
crates/sketch-intake/target/debug/openwam request /tmp/inspect-post.json
```

Generation rebuilds source geometry and catalog selection, then binds commands,
text, summary and blockers to project/source/table digests. Verification regenerates
and compares every candidate field, including raw text and eligibility flags.
Inspection returns a typed program, canonical serialization and feed/dwell summary.
Canonical serialization preserves semantics, not comments or original formatting.
Successful inspection does not qualify controller compatibility.

Reproduce the captured client's static string mapping:

```sh
cargo run --locked --manifest-path crates/wam-client-inspect/Cargo.toml -- wazer/private/public-assets/main-SLLISYKG.js > /tmp/wam-string-map.json
```

The tool rejects other hashes and requires a unique rotation. It is a
capture-specific research instrument, not a JS interpreter.

## Recorded evidence

[The demonstration receipt](wazer-postprocessor-demonstration.json) binds the
capture, historical exports and retained 8HP panel candidate. The gauge has four
paths, 778 linear moves and one unresolved initial feed. The nozzle guide has four
paths, 309 linear moves and no unresolved feed. Both pass semantic round trips.
The 8HP candidate has six paths and 216 linear moves with explicit feeds; every
cutting coordinate and feed matches its Swarf draft. Independent verification
passes while hardware eligibility remains false. Tests exercise changed code,
changed provenance and forged eligibility through the actual CLI.

The [comparison harness](wazer-parity.md) now measures byte, command and resolved
trace agreement. A same-input WAM CAM fixture remains required for generator parity.

Original generator services also run in a [Node oracle](wam-node-oracle.md), with
independent retained output and measured differences against the Rust square.

The explicit `wam24_pro_presegmented` profile is now available for one closed
Pro service path. See [differential corpus and supported scope](wam-pro-parity.md).
