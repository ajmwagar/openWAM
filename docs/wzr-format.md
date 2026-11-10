# WAM 2.4 `.wzr` project format

Reverse-engineered from three local files (not committed; the opt-in tests read
them from `OPENWAM_WZR_SAMPLES=<dir>`):

| Sample | Writer | Shows |
|---|---|---|
| **F** `Foam Insert.wzr` (11,585 B) | WAM 2.4.0 | 1 file, 3 instances, scale 1.8647, `Inside`, tabs `automatic` + `none`, lead geometry, Pro user material |
| **E** `wazerjob (1).wzr` (9,304 B) | WAM 2.4.0 | 2 files (px SVG + Inkscape mm SVG), 5 groups, `Centerline`, mixed instance scales, an exponent number (`3.158660000224245e-8`) |
| **P** `wzr-forge-part-single-instance-2026-11-08.wzr` (2,464 B) | 2026-11-08 prototype, not WAM | the shape a non-WAM writer produced; no `wamVersion`/`cutOrder`/`displayUnit`, no `original*` box fields |

F and E came from the `WAM/` folder of the SD card in titan's card reader;
the card also holds unrelated camera files. No source DXF/SVG sits next to them,
but each `.wzr` embeds its design, so every sample is its own design/project pair.

## What a `.wzr` is (and is not)

A `.wzr` is the **WAM web app's saved project**, `JSON.stringify(state)`. It is
not a cut file. It has **no toolpath, no motion commands, no pierce sequence,
no pressure/abrasive on/off events, no header/footer, and no checksum**. WAM
recomputes offsets, leads, pierces and G-code from it when the project is
opened. Consequences:

- Format confidence for read/write is high: plain JSON, every key and value type
  catalogued below, 3/3 samples round-trip byte-identically.
- "Same command sequence" cannot be compared from a `.wzr`; the golden test
  compares project structure and geometry instead.
- Producing a `.wzr` does not produce machine output.

## Encoding (F, E, P)

UTF-8, no BOM, no trailing newline, compact separators (`,` `:`), keys in the
order below, strings escaped as `JSON.stringify` does (`\"`, `\\`, `\n`; no `\/`,
no `\u` for non-ASCII). Numbers use ECMAScript `Number::toString`: integers have
no `.0` (`150`, `0`), shortest round-trip digits (`26.20000000000001`), exponent
form below 1e-6 with no zero padding (`3.158660000224245e-8`, E). `-0` prints `0`.
NaN/Infinity never appear (they would print `null`); the writer rejects them.

## Top level (key order)

| Key | Type | Notes |
|---|---|---|
| `wamVersion` | string | `"2.4.0"` (F, E); absent in P |
| `fileList` | object | `{files: [File], fileIDCounter: int}`; counter > every `fileID` (E: IDs 4, 11, counter 12) |
| `selectedMaterial` | object | `{material, quality, abrasiveMode}` |
| `offsets` | [Offset] | one per group |
| `tabs` | [Tab] | one per group |
| `cutQuality` | enum | equals `selectedMaterial.quality` in every sample |
| `cutOrder` | object | `{mode: "automatic", groups: [{pathGroupID, segments: [null]}]}`, one per group; absent in P |
| `displayUnit` | `"in"` | absent in P |
| `placements` | [Placement] | one per file |

## File

`filename`, `fileID`, `units` (`"mm"` in all samples, even px SVGs; it does not
change the box rule), `svgString` (the whole SVG text), `splitGroups` (`false`
everywhere; `true` is rejected as unseen), `groupInstances` (`[[g0, g1, …]]`:
one group ID **per placed instance** of the whole file, F: `[[0,1,2]]`),
`instances` (= that list's length), `fileBB`.

### `fileBB` rule (verified: F, E to 9e-16 in)

- Only `<path>` elements count. F's SVG has two `<rect>` and two `<circle>`
  elements plus one `<path>`; WAM's box (398–537 × 296–434 px) is the path alone.
  So **WAM 2.4 imports `<path>` only**; the exporter converts every cut contour
  to `<path>`.
- Stroke width is ignored; curve extrema are exact.
- User units map to CSS px through the root `width` / `viewBox` ratio, then
  inches = px / 96. E `Ellipse 1.svg` (`width="206"`, px): 0.5 px → 0.005208…;
  E `wam.svg` (`width="210mm" viewBox="0 0 210 297"`): 185.10638 → 7.28765… in.
  With no `width` (P) user units are px.
- `original{Min,Max}{X,Y}` are written as `0` in `fileBB`/`baseBB`.

## Material (`selectedMaterial.material`, key order)

`userDef` bool, `uId` int|null (F/E: user material `true`/257; P: `false`/null),
`name`, `units` (`in`|`mm`), `category`, `thickness`, `speedMode`, `ecoMode`,
`pierceTime`, `defaultTabWidth`, `defaultLeadDist`, `machineType`
(`pro`|`desktop`), `maxTab` (int; present for `pro`, absent for P's `desktop`).
Every quantity is a math.js unit `{"mathjs":"Unit","value":n,"unit":"…","fixPrefix":false}`.
`speedMode`/`ecoMode` = `{name: "Speed Mode"|"Eco Mode", abrasiveRate (g / min),
roughRate, mediumRate, fineRate (in / min | mm / min)}`. Abrasive and pressure
are not separate fields: pressure mode is `abrasiveMode` (`speedMode` |
`ecoMode`), quality picks the feed (`roughRate` | `mediumRate` | `fineRate`).

## Offset

`{groupID, type, distance, fileID}`. `type`: `Inside` (F), `Centerline` (E);
`Outside` is accepted but **unseen**. `distance` is `0.019` (inches) in every
sample regardless of material.

## Tab (key order)

`groupID`, `fileID` (absent in P), `type` (`automatic` | `none`), `tabCount`,
`tabSize` (in), `enableLeads`, `cutType` (`standard`), `scoredTabSpeedMultiplier`
(3), `tabLocations`. `type: "none"` stores `[[]]` (F group 2). `automatic` stores
WAM's computed cache: `[[{tabStart, tabEnd, tabID, tabSize, cutType,
scoredTabSpeedMultiplier}]]`. Tab ends are `{x, y, pathDistance, pathSegment,
lead, cutType, scoredTabSpeedMultiplier}` in inches, centred on the RSTBB origin,
y down (F: the circle top is y = −1.32). `lead` is `null` (E) or a lead-in/out polyline `{points, closed: false,
baseBB (all null + original 0s), center}` (F): a quarter circle of 10 points
whose radius is `defaultLeadDist·2/π` (0.15 in → 0.0955 in) and whose
`pathDistance` runs 0 → radius in 9 equal steps.

## Placement

`{fileID, baseBB, RSTBB, instances: [{scale, rotation, position: {x, y}}]}`.
`baseBB` = `fileBB`. `RSTBB` = the box scaled by **instance 0's** scale and
centred near the origin, with `original*` equal to its own min/max (F: width
2.7 = 1.4479 × 1.8647). `position` is where the RSTBB origin lands on the bed in
inches (F: 4.15003 − 1.35003 = 2.8 in left edge). WAM's centre is not exactly the
box centre (offset ≤ 3.4e-5 in, F). Instances with another scale (E: 1 vs
1.639) still share instance 0's RSTBB.

## Reader/writer (`crates/openwam-wzr`, MIT OR Apache-2.0)

Typed serde structs in WAM key order, `deny_unknown_fields`, enums limited to
observed values (+ `Outside`), `[null]`-only cut-order segments, cross-reference
checks (groups ↔ offsets ↔ tabs ↔ cutOrder ↔ placements). Writer = serde_json
with an ECMAScript number formatter. Unknown keys/values fail with typed errors;
nothing is repaired.

## Export from openWAM (`openwam-wzr export_wzr`)

Input: a `plan_cam` artifact. The command runs `openwam request -` `verify_cam`
itself (separate process; the AGPL crate is never linked) and refuses unless
`verified: true` for the same `project_sha256`. Output: one file, one instance.

| Field | Source |
|---|---|
| `svgString` | cut contours only (no-cut contours dropped), as `<path>` in mm, `width/height` in mm |
| `fileBB`/`baseBB` | rule above on that SVG |
| offset `type` | `Outside` when every contour keeps the part (even depth outside, odd inside), `Inside` for the inverse, else refused (`MixedSides`) |
| offset `distance` | WAM's `0.019` |
| material | openWAM catalog row: speed feeds = row feed for rough/medium/fine, pierce = row, abrasive = machine `abrasive_kg_min`, lead = intent max, `userDef: true`, `uId: null`, category `openWAM` |
| tabs | `none`, `[[]]`, `enableLeads` when the intent has leads |
| position | openWAM `placement_xy_down_mm` + box centre, inches |

### Unknowns (flagged in every export report)

1. `uId: null` with `userDef: true` has not been opened by WAM.
2. Bed origin/axis direction of `position` (assumed top-left, y down).
3. `Outside` offset type is unseen in a WAM file.
4. Eco-mode feeds = speed feed × 0.0947 (F's Pro ratio), only to keep eco slower.
5. openWAM tabs are not carried (WAM's tab cache needs WAM's own path geometry).
6. `splitGroups: true`, rotation ≠ 0 and non-`standard` cut types are rejected.

One WAM-saved file would close 1–3: export a part, open it in WAM, move nothing,
save it again, and diff it with `openwam-wzr read`.

## Usage

```sh
cargo build --release -p openwam-wzr -p reversal-sketch-intake --bin openwam --bin openwam-wzr
target/release/openwam-wzr read  sample.wzr        # normalised summary + fileBB recomputation
target/release/openwam-wzr roundtrip sample.wzr    # identical: true|false, first_diff_offset
target/release/openwam-wzr export_wzr --artifact plan_cam.json --out part.wzr --name part.svg
OPENWAM_WZR_SAMPLES=dir OPENWAM_CLI=target/release/openwam cargo test -p openwam-wzr
```

Export refuses removable-media destinations (`/run/media`, `/media`, `/mnt`)
and never overwrites an existing file.
