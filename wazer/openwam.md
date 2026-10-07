# OpenWAM: offline project preparation

The [agent API and JSON CLI](openwam-api.md) are the primary interface. Product
frontend work follows that contract and will use Svelte/Coherence. The browser
prototype below is retained as an experiment, with no further UI expansion here.

OpenWAM is the independent local preparation workflow built on the existing Rust
sketch adapter, Transmog geometry types, Swarf waterjet compiler and controller
simulator. It requires no WAM login, serial-number activation, subscription,
hosted service or network connection at runtime. Building from source initially
requires Cargo dependencies and the sibling owner checkouts described in
[integration.md](integration.md). After building, the binary includes the captured
cutting-table snapshot.

## Create, reopen, preview and simulate

### Experimental browser preview

```sh
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml --features ui --bin openwam-preview-ui -- 0
```

Open the address printed at startup. Port `0` asks the OS for a free local port;
you can provide a fixed port instead. Upload a drawing, select the exact machine/material/
thickness/unit table point, set placement and curve tolerance, then prepare the
preview. The project page lets you revise settings, save the portable JSON
project, open the full preview and download a synthetic simulator replay. Revisions
preserve earlier projects. Use the home page to reopen a saved project, including
in a new app session. Stop the app with Ctrl-C.

The interface uses server-rendered HTML forms with no JavaScript, CDNs, analytics
or external resources. Axum owns HTTP/multipart parsing; the existing Rust project
and owner APIs perform planning and recomputation. A local session token and
Host/Origin checks protect submissions. It binds only to IPv4 loopback, accepts
only its advertised `127.0.0.1` origin, serializes requests and bounds upload/session
memory. Up to 16 unique project revisions and 128 MiB are retained in memory.
Save files before stopping/restarting; the app does not silently persist drafts
to your filesystem. This is a local desktop workflow, not a network service.

The optional `ui` feature keeps the HTTP runtime out of the ordinary CLI build.
The live HTTP workflow (home, prepare, editor, save, preview, simulate and reopen)
passed with the plate drawing; all 125 operations completed in the simulator.
The [UI demonstration receipt](openwam-ui-demonstration.json) retains artifact
digests and scope. All 38 tests and Clippy passed. Browser visual inspection was
unavailable; HTML structure and numerical preview coordinates were tested.

Application code uses the crate's MIT/Apache-2.0 licensing, as does Transmog.
Both the CLI and UI also link Swarf's AGPL-3.0 waterjet library and the
GPL-3.0-or-later controller simulator. These source licenses remain intact;
redistributions must include the applicable notices and corresponding source.
See [Swarf's license](https://github.com/FuturePresentLabs/swarf/blob/088920228442369bef723ae75b60af3e5788f880/LICENSE) and the controller's
[COPYING](../crates/waterjet-control/COPYING). No proprietary WAM JavaScript is
incorporated into the implementation.

### CLI and manual parity

From Reversal's root:

```sh
cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml
crates/sketch-intake/target/debug/reversal-sketch-intake --openwam-create crates/sketch-intake/examples/plate.svg pro Aluminum 3 metric 0.02 0 0 > /tmp/plate.openwam.json
crates/sketch-intake/target/debug/reversal-sketch-intake --openwam-check /tmp/plate.openwam.json
crates/sketch-intake/target/debug/reversal-sketch-intake --openwam-preview /tmp/plate.openwam.json > /tmp/plate-openwam.html
crates/sketch-intake/target/debug/reversal-sketch-intake --openwam-simulate /tmp/plate.openwam.json > /tmp/plate-openwam-simulation.json
```

Open the HTML in a browser. It is self-contained, contains no scripts or external
assets, and uses the verified operations to show cutting paths, travel and
pierce locations. Coordinates are displayed with the bed's top-left origin and
Y down. Material thickness, nominal feed, pierce time, kerf and path estimate are
shown alongside the original source digest. The estimate excludes travel and
hydraulic sequencing; simulation has separate synthetic timing assumptions.

The retained plate demonstration reopened successfully and completed all 125
operations in 207,158 virtual milliseconds. Digests and scope are recorded in
[openwam-demonstration.json](openwam-demonstration.json). Validation passed 34
tests, including four project/preview tests, plus Clippy and formatting checks.
The preview's HTML structure and operation coordinates were tested; visual
inspection was unavailable because no browser was connected.

The same creation command accepts explicitly scaled planar `.dxf` inputs. Its
arguments match the owner-integration command: model, exact material/thickness
table point, unit system, curve tolerance in mm and placement X/Y-down in mm.
An unsupported material point or invalid geometry fails before any project is
printed. The existing importer limits remain in force.
SVG contours currently require author IDs for the Swarf bridge; support for
anonymous contours with explicit source-bound locators is tracked in
`reversal-wey`. The UI reports that importer/planner failure visibly.

## Persistence contract

`reversal.openwam-project.v1` retains the exact UTF-8 drawing in `source_text`
and one `integration` report. The report owns selection, placement, tolerance,
source/table digests, Transmog features and Swarf operations; project settings are
not copied into another mutable structure. Reopening requires no original file
path and independently rebuilds the entire report through the existing owners.
Changed source, forged operations, altered tables or extra report fields fail.
The outer schema rejects unknown fields and unknown versions. Original input is
bounded to 1 MiB, project input to 64 MiB. JSON escaping preserves line endings.

This verifies consistency with the drawing, installed table snapshot and owners;
it is not a signature or proof of who authored the project. Deliberate revisions
can be made by creating a new project. Table/owner changes that affect the report
fail reopening rather than silently changing an old job. Keep the compatible
binary/source revision alongside long-lived projects until a migration contract
is implemented. Project files contain the full drawing; share them accordingly.

## Compatibility work

This release supplies a local import/settings/placement/save/reopen interface,
a CLI and browser preview, not a complete WAM replacement. Drafts preserve source
contour order and centrelines. Kerf offsets,
leads, tabs, contour joining/topology and hole-first ordering remain unqualified.
The [Rust postprocessor](wazer-postprocessor.md) now emits explicitly unqualified
research G-code through the agent API and independently verifies candidates.
There is no SD transfer or flashing; machine output stays disabled. The
simulator's physical model remains synthetic.

The next work is owner-provided CAM transformations and controller qualification
for the versioned postprocessor, starting from the historical reference exports.
These are tracked in
Beads rather than hidden behind a vendor login requirement.

WAZER's current public software page calls WAM free; its support procedure ties
full accounts to a software serial number. A recurring-payment condition has not
been established by this research. Hosted access still remains an external
dependency that this local workflow avoids:
[software page](https://wazer.com/waterjets/software),
[account registration](https://support.wazer.com/resources/maintenance/procedures/register-wam-account).
