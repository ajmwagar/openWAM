# OpenWAM

An independent, offline waterjet CAM toolkit in Rust. Import SVG/DXF, select
source-pinned WAZER material settings, retain project intent and independently
verify derived plans. The primary interface is an API and JSON CLI for agents
and applications.

## What works

- Physical units, SVG transforms and curves, bounded planar DXF intake.
- Stable document-local identities for anonymous and compound SVG paths.
- Explicit inside/outside/centerline/no-cut policy and kerf overrides.
- Holes-first ordering, selectable starts, uncut tabs and waste-side leads on
  every separately pierced span.
- Portable source/intent/table-bound artifacts, independent recomputation and
  standalone SVG previews.
- Synthetic control/plant fault replay and explicit research G-code profiles.
- Verified CAM to [LinuxCNC waterjet research G-code](wazer/linuxcnc-waterjet.md),
  with explicit jet/abrasive outputs, process delays and target travel/feed limits.
- Source-bound exported-command replay with explicit dwells and separate jet /
  abrasive sequencing; see [the replay runbook](wazer/gcode-simulation.md).
- Pinned Pro firmware [shutdown research](wazer/shutdown-research.md) with
  byte-checked command, completion and pause/abort evidence.
- [Switch/HALT research](wazer/switch-halt-research.md) covering command exclusions,
  ordered HALT writes, config defaults and the watchdog/debugger evidence boundary.
- [Shared shutdown timer research](wazer/shared-shutdown-timer.md) pins mode,
  completion and pause timer ownership, state guards and unresolved callbacks.
- [Offline shutdown replay](wazer/shutdown-replay.md) with six-channel traces,
  strict timer boundaries, output faults and agent API comparison/verification.
- [G-code completion integration](wazer/gcode-shutdown.md) derives M1404 dispatch
  from verified command replay and retains emitted dwells and internal timers separately.

The CAM planner currently supports simple closed flattened polygons with bounded
miter offsets. Unsupported topology fails explicitly. Verified CAM artifacts feed
the simulator and multi-span historical research postprocessor, with independently
recomputed retained replay/export artifacts. All machine-output flags remain
false; no machine transfer or qualified controller export is implemented.

## Start with the CLI

```sh
cargo run --locked -p reversal-sketch-intake --bin openwam -- capabilities
cargo run --locked -p reversal-sketch-intake --bin openwam -- request request.json
```

Use [the agent API](wazer/openwam-api.md) for request/response schemas and
[the contour CAM runbook](wazer/independent-cam.md) for a complete panel example
with kerf, tabs and leads. Original sources are retained; artifacts verify by
rebuilding from source and typed intent, without a WAM account or runtime network.

Crate names retain their existing `reversal_*` identities to preserve the Rust
API and portable JSON protocols during extraction. The product UI will use
Svelte/Coherence over this API. The optional Rust preview UI is a historical
experiment, not the product frontend.

Minimal Transmog and Swarf source dependencies are pinned under `vendor/` so the
checkout builds without sibling repositories. These are owner snapshots, not
independently maintained implementations. See [dependency provenance](vendor/README.md).

## Develop

```sh
cargo check --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

## License and contributions

OpenWAM first-party application code is **AGPL-3.0-or-later**. See [LICENSE](LICENSE)
and [third-party notices](NOTICE.md) for dependency and historical source licenses.
This project is independent of WAZER and is not manufacturer-certified.

Contributions use the same copyright-assignment agreement as
[ajmwagar/pedalkernel](https://github.com/ajmwagar/pedalkernel/blob/main/CLA.md).
Read [CLA.md](CLA.md) and [CONTRIBUTING.md](CONTRIBUTING.md) before contributing.
