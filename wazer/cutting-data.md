# WAZER cutting data and the offline planning loop

The Rust loop now runs **SVG → exact table selection → job estimate → retained
JSON report → independent recomputation**. It produces a planning artifact, not
a controller cut file. Geometry execution and Swarf postprocessing remain
separate, unqualified stages.

The 2026-10-06 snapshot covers all 142 thickness rows in WAZER's captured public
[materials table](https://wazer.com/materials/materials-specs/), across 21 materials.
[cutting-tables.json](cutting-tables.json) contains 568 entries: each source row
has Pro/Desktop and metric/imperial variants. It preserves 80 unsupported model
entries and flags four variants of one anomalous source row. These are snapshot
coverage counts, not a claim to the full authenticated WAM library.

The [acquisition receipt](cutting-data-acquisition.json) pins the original pages,
normalized dataset and one response from the page's declared calculator API.
The importer uses HTML selectors and validates model columns, units, finite
positive numbers, duplicates and complete source-row variants. Missing columns,
conflicting repeated specs, denied HTML and incomplete imports fail explicitly.
The bundled loader checks dataset bytes and provenance against the receipt.
Original pages/API response stay in ignored `private/cutting-data/`.

## Use the tables

From Reversal's root:

```sh
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --cutting-list Aluminum
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --plan-wazer crates/sketch-intake/examples/plate.svg pro Aluminum 3 metric > /tmp/plate-job.json
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --verify-wazer crates/sketch-intake/examples/plate.svg /tmp/plate-job.json
```

For imperial selection, use `0.125 imperial`. It selects the imperial published
point, rather than the 3 mm point. Material matching is case-insensitive and exact;
`Aluminum 6061` does not silently select generic `Aluminum`. Unlisted thicknesses
and explicitly unsupported model combinations fail. No interpolation or automatic
maximum-thickness substitution is performed.

Each report retains the complete mm sketch, original SVG digest, canonical JSON
digest of the typed table snapshot, selected source-row identity and machine
limits. The verifier reparses the original SVG and reruns table selection and
estimation. It rejects changed geometry, table settings, forged feed/time values,
extra fields and altered output eligibility; supplied hashes are not authority.

## Parameter meaning

| Parameter | Source and treatment |
|---|---|
| Material/thickness | Exact public row and original unit system; alloy/grade is not inferred |
| Feed | Published IPM or mm/min, converted to both representations from the selected point |
| Pierce time | Published model/unit value, normalized to seconds |
| Kerf | Nominal machine spec; metric and imperial labels remain separate |
| Side offset | Kerf / 2 reported as a parameter; geometry has not yet been offset |
| Cutting area / gantry maximum | Published model/unit specs; conservative control-point bounds and feed overruns are reported |
| Abrasive rate / hopper / continuous time | Published model/unit specs; nominal abrasive estimate and refill diagnostics |
| Nozzle/orifice, mixing tube and positional accuracy | Retained source spec values; orifice diameter is not treated as kerf |
| Quality, corner policy, leads, tabs | Unknown from the public table; no inferred calibration values |

Public metric/imperial entries are separate nominal material points, not exact
conversions: thickness, feed and piercing may all differ. Machine specs also use
rounded labels: 0.044 inches converts to 1.1176 mm while the metric label is
1.1 mm. Both are retained, neither is called an exact-unit measurement. A source
Brass row pairs 0.0125 inches with 3 mm. Its four variants remain searchable but
cannot be selected for planning because the paired thicknesses differ roughly tenfold.

Curved-path lengths use `kurbo` on normalized SVG quadratic/cubic curves, with
0.001 mm arclength tolerance per curve. Straight lengths and explicit closures
are summed directly. Nominal time is raw centreline length/feed plus one pierce
per retained contour. Nominal abrasive mass uses that time and the selected
machine's published abrasive rate. These estimates exclude kerf-offset geometry,
lead/tab changes, travel, acceleration, corners, catch-up delays and maintenance.
Open contours and area/feed/refill issues are explicit diagnostics.

The [calculator crosscheck](calculator-crosscheck.json) records one read-only
public calculation for Aluminum at 0.125 inches. Both machines' displayed feed
and pierce values match their public imperial table entries. The API's internal
rate is more precise than the displayed rate, so nominal estimates are not exact
calculator/WAM timing. Arbitrary-thickness formulas and current WAM library
versions have not been recovered.

`machine_output_enabled` remains false. Existing owner work is tracked in
`reversal-l3u` (Transmog curve/DXF bridge) and `reversal-uhn` (Swarf waterjet
operations/postprocessor). The controller's commands and startup/shutdown delays
are version-dependent according to
[WAZER's G-code reference](https://support.wazer.com/resources/maintenance/wam-procedures/understanding-g-code-on-wazer).
The selected export target is **WAZER Pro**, confirmed by the user on 2026-10-06.
The user will provide its firmware version. That version and reference WAM
exports are needed to verify the postprocessor stage; no controller version or
WAM version is invented in these reports.

## Reproduce the import

```sh
cargo run --locked --manifest-path crates/sketch-intake/Cargo.toml -- --import-wazer 2026-10-06 wazer/private/cutting-data/materials-specs.html wazer/private/cutting-data/tech-specs.html > /tmp/wazer-cutting-tables.json
cmp /tmp/wazer-cutting-tables.json wazer/cutting-tables.json
cargo test --locked --manifest-path crates/sketch-intake/Cargo.toml
```

Use the retained source bytes for a historical replay; a fresh fetch is a new
snapshot requiring a new acquisition date/digest and inspection of schema/data
changes. The calculator API request and exact response digest are in the receipt.
All table/planning commands work offline after build.
