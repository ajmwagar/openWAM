# OpenWAM execution receipt — 2026-11-07

This is an observed execution record, not a design document. Every number below was
produced by running the CLI in this checkout and saving the request and response on
disk. The files live next to this receipt in
[`receipts/2026-11-07/`](receipts/2026-11-07/).

It exists because the ladder in [openwam-api.md](openwam-api.md),
[independent-cam.md](independent-cam.md) and [cam-loop.md](cam-loop.md) was
documented operation by operation but had never been run end to end against the
repository's own fixtures in a single saved session.

## Environment

| Item | Observed value |
|---|---|
| Repository | openWAM at `c144ef2dc75e89d2d1e6c0cdbfd35df69af5e470` (`== origin/main`) |
| Worktree / branch | `.worktrees/wam-receipt` on `pi/wam-receipt` |
| Build | `cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam` |
| Binary | `target/debug/openwam`, `package_version: "0.1.0"` |
| Toolchain | `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1 (797e8a9bc 2026-08-05)` |
| Protocol | request `reversal.openwam-request.v1`, response `reversal.openwam-response.v1` |
| Network | none used; every rung is a local process invocation |
| Run window | 2026-11-07 17:41–17:42 host local time |

Input source (unmodified, the example shipped with the crate):

```
b70417041f210eb8f1da16b405b4cef8b932459ddc0ec19ec1595a26e63cbc2a  crates/sketch-intake/examples/plate.svg
```

The prepare response echoes that digest as `transmog.source_sha256` and the verify
receipt echoes it as `source_sha256`, so source identity is bound end to end.

## Rung summary

All eight rungs exited **0**. No rung produced a structured error (exit 2) and no
rung lost its output channel (exit 3). Those two exit paths are exercised separately
in [Negative controls](#negative-controls) so the receipt does not merely assert the
contract's exit-code semantics.

| # | Operation | Exit | Status / kind | Principal observed numbers | Saved files |
|---|---|---|---|---|---|
| 1 | `capabilities` | 0 | ok, `reversal.openwam-capabilities.v1` | `machine_output_enabled: false`, `hardware_qualified: false`, limits `{source 1048576 B, project 67108864 B, request 69206016 B, response 138412032 B}` | `01-capabilities.response.json` |
| 2 | `prepare` | 0 | ok, `project` | 3 features, 125 nominal operations, 3 pierces, raw centerline 336.2281088577347 mm, estimate 204.7933938098526 s, kerf 1.1 mm, side offset 0.55 mm, feed 108 mm/min, tolerance 0.02 mm, placement `[0,0]` | `02-prepare.request.json`, `02-prepare.response.json` |
| 3 | `verify` | 0 | ok, `verification` | `verified: true`, `operation_count: 125`, 3 blockers (centreline draft, unqualified controller, open contour policy) | `03-verify.request.json`, `03-verify.response.json` |
| 4 | `plan_cam` | 0 | ok, `cam` | 2 cut paths, 4 cut spans, 90 operations (78 cut / 4 pierce / 4 rapid / 4 stop), nominal 190.86064895702012 s, raw centreline 300.34916812263623 mm, kerf basis `request_published_nominal` | `04-plan_cam.intent.json`, `04-plan_cam.request.json`, `04-plan_cam.response.json` |
| 5 | `verify_cam` | 0 | ok, `verification` | `verified: true`, `path_count: 2`, `operation_count: 90` | `05-verify_cam.request.json`, `05-verify_cam.response.json` |
| 6 | `preview_cam` | 0 | ok, `preview`, `image/svg+xml` | 16 542 characters of SVG content (16 543-byte saved file), 460×305 mm stock frame, 78 cyan cut moves, dashed grey rapids, 3 grey source boundaries, 4 orange pierce markers | `06-preview_cam.request.json`, `06-preview_cam.response.json`, `06-preview_cam.artifact.svg` |
| 7 | `research_cam_gcode` (`historical_wam16`) | 0 | ok, `cam_research_gcode` | 125 commands, 4 paths, 78 linear, 4 rapids, 36.0 s total dwell, 166.86064895702017 s known feed cut, 0 unresolved feeds, declared version 1.6, `controller_qualified: false` | `07-research_cam_gcode.request.json`, `07-research_cam_gcode.response.json`, `07-research_cam_gcode.artifact.nc` |
| 8 | `verify_cam_gcode` | 0 | ok, `verification` | `verified: true`, `controller_qualified: false`, regenerated `gcode_sha256` matches rung 7 | `08-verify_cam_gcode.request.json`, `08-verify_cam_gcode.response.json` |

Minimum requested ladder — prepare → verify → plan_cam → verify_cam → preview_cam —
is green with saved artifacts. Both research G-code rungs are green as well and
independently verified; they remain research exports, not controller output.

## Exact commands

Run from the worktree root. `R` and `C` are shorthand for the receipt and control
directories.

```sh
cargo build --locked --manifest-path crates/sketch-intake/Cargo.toml --bin openwam
R=wazer/receipts/2026-11-07
C=$R/controls
mkdir -p "$R" "$C"

# 1 capabilities
./target/debug/openwam capabilities > "$R/01-capabilities.response.json"

# 2 prepare (source bytes taken verbatim from the fixture, settings from the goal)
jq -n --rawfile source crates/sketch-intake/examples/plate.svg \
  '{schema:"reversal.openwam-request.v1",request:{operation:"prepare",source_text:$source,source_format:"svg",settings:{model:"pro",material:"Aluminum",units:"metric",thickness:3,tolerance_mm:0.02,placement_xy_down_mm:[0,0]}}}' \
  > "$R/02-prepare.request.json"
./target/debug/openwam request "$R/02-prepare.request.json" > "$R/02-prepare.response.json"

# 3 verify project
jq -n --slurpfile p "$R/02-prepare.response.json" \
  '{schema:"reversal.openwam-request.v1",request:{operation:"verify",project:$p[0].result.project}}' \
  > "$R/03-verify.request.json"
./target/debug/openwam request "$R/03-verify.request.json" > "$R/03-verify.response.json"

# 4 plan_cam with explicit per-contour intent (written out below)
jq -n --slurpfile p "$R/02-prepare.response.json" --slurpfile i "$R/04-plan_cam.intent.json" \
  '{schema:"reversal.openwam-request.v1",request:{operation:"plan_cam",project:$p[0].result.project,intent:$i[0]}}' \
  > "$R/04-plan_cam.request.json"
./target/debug/openwam request "$R/04-plan_cam.request.json" > "$R/04-plan_cam.response.json"

# 5 verify_cam
jq -n --slurpfile p "$R/04-plan_cam.response.json" \
  '{schema:"reversal.openwam-request.v1",request:{operation:"verify_cam",artifact:$p[0].result.artifact}}' \
  > "$R/05-verify_cam.request.json"
./target/debug/openwam request "$R/05-verify_cam.request.json" > "$R/05-verify_cam.response.json"

# 6 preview_cam
jq '.request.operation="preview_cam"' "$R/05-verify_cam.request.json" > "$R/06-preview_cam.request.json"
./target/debug/openwam request "$R/06-preview_cam.request.json" > "$R/06-preview_cam.response.json"
jq -r '.result.content' "$R/06-preview_cam.response.json" > "$R/06-preview_cam.artifact.svg"

# 7 research_cam_gcode
jq -n --slurpfile p "$R/04-plan_cam.response.json" \
  '{schema:"reversal.openwam-request.v1",request:{operation:"research_cam_gcode",artifact:$p[0].result.artifact,profile:"historical_wam16"}}' \
  > "$R/07-research_cam_gcode.request.json"
./target/debug/openwam request "$R/07-research_cam_gcode.request.json" > "$R/07-research_cam_gcode.response.json"
jq -j '.result.candidate.code.gcode' "$R/07-research_cam_gcode.response.json" \
  > "$R/07-research_cam_gcode.artifact.nc"

# 8 verify_cam_gcode
jq -n --slurpfile p "$R/04-plan_cam.response.json" --slurpfile g "$R/07-research_cam_gcode.response.json" \
  '{schema:"reversal.openwam-request.v1",request:{operation:"verify_cam_gcode",artifact:$p[0].result.artifact,candidate:$g[0].result.candidate}}' \
  > "$R/08-verify_cam_gcode.request.json"
./target/debug/openwam request "$R/08-verify_cam_gcode.request.json" > "$R/08-verify_cam_gcode.response.json"
```

Use `jq -j`, not `jq -r`, when extracting the G-code for hashing. `jq -r` appends a
newline, which changes the file digest: the first extraction here used `-r` and
hashed to `247bdf3af31b9362180f8fb7c6abb77f7f14e99a46c91a9f6d9851839a8e31a3`,
which does not match the candidate's own `gcode_sha256`. Re-extracting with
`jq -j '.result.candidate.code.gcode'` yields the saved artifact above, whose file
digest is exactly `d87d66ec…`. The candidate hash is therefore over the returned
string, not over a newline-terminated file, and the retained `.nc` here is the
byte-exact string. (The code already carries CRLF line endings, so 126 lines.)

### Intent used for `plan_cam`

`plate.svg` imports three contours: `outline` (closed, 90×40 mm at 5,5), `hole`
(closed circle r=5 at 20,25) and `open-sketch` (open cubic, `fill="none"`). The
intent below is exactly the file saved as `04-plan_cam.intent.json`:

```json
{
  "kerf_override_mm": null,
  "contours": [
    {"source_id": "outline", "side": "outside", "start_fraction": 0, "tabs": [{"center_fraction": 0.125, "width_mm": 3}, {"center_fraction": 0.625, "width_mm": 3}], "lead_in_mm": 2, "lead_out_mm": 2},
    {"source_id": "hole", "side": "inside", "start_fraction": 0, "tabs": [], "lead_in_mm": 1, "lead_out_mm": 1},
    {"source_id": "open-sketch", "side": "no_cut", "start_fraction": 0, "tabs": [], "lead_in_mm": 0, "lead_out_mm": 0}
  ]
}
```

Observed plan geometry: `hole` at containment depth 1, inside, one span, perimeter
27.949164927827287 mm, cut first; `outline` at depth 0, outside, three spans split by
the two 3 mm tabs, 2 uncut intervals, perimeter 264.40000319480896 mm. Every span got
its own rapid/pierce/cut/stop sequence, so four pierces at 6 s each are folded into
the 190.86 s estimate. The open annotation stayed in the project and the preview as
source context; it was not cut.

## Artifact digests

Computed with `sha256sum` over the files exactly as saved.

| File | Bytes | SHA-256 |
|---|---|---|
| `01-capabilities.response.json` | 60 769 | `411c61da4422957de6bf908410332bf168a6a0602d1c4201ada3ab8fe447e4e6` |
| `02-prepare.request.json` | 660 | `6fb84fea5767f8dc8d55697c33dc33eb7761202f206475c50cd3de69956fc521` |
| `02-prepare.response.json` | 39 812 | `0da6c41887647b8bcf9285008dff1e258d3118f2adf858bf661ef8d228e560a7` |
| `03-verify.request.json` | 123 991 | `a964c06662a3ad2c53c13649c333aee75ed835f0d5d4c52573e1f921e2a7c11f` |
| `03-verify.response.json` | 648 | `89dd60aa3745da21064630983b817f5ef09f57817a3507f8c90103761be589b7` |
| `04-plan_cam.intent.json` | 485 | `3348d46b0404c830c4d625111d1a5902a51ebc92bfbc65f9e316b72ca79b1086` |
| `04-plan_cam.request.json` | 124 851 | `9f550d6ff2293101809f6350790d94b3e876488fc5d51b74ccccf859fc621aaf` |
| `04-plan_cam.response.json` | 63 862 | `3f970b1d9ceb6c98708fac012da4667d648774c4aa732122f3d9756469431578` |
| `05-verify_cam.request.json` | 200 337 | `3586321a7158cf83cc794027eacf4c4a2857d178f00ba3a8e5295f07c1400b66` |
| `05-verify_cam.response.json` | 352 | `2042af9013c6fb5543f46f9099ad2bf3a34223b136a93d23a85639c8d9065be7` |
| `06-preview_cam.request.json` | 200 338 | `2caea10e41032cca0f2efb58b2aa24185f1ddd155a3aa0df22f651faf87417a3` |
| `06-preview_cam.response.json` | 17 423 | `6951b1686a10b3f022030f6cc4a0ce262fd75338f44425698e621ade93de99f0` |
| `06-preview_cam.artifact.svg` | 16 543 | `2a2bcd48bd72be9a97d9c8cb9286115913579ee31e4bfebe02729b13f321c216` |
| `07-research_cam_gcode.request.json` | 200 380 | `98b9b8a8b725c1ba825d65aa1576871a8adb1c99ce3fd2fcb3ae53ae7d7d1c02` |
| `07-research_cam_gcode.response.json` | 15 398 | `11db4be24b4a4de5b80532edacb668ddd035a54ac2db938093dfb50c99577ab5` |
| `07-research_cam_gcode.artifact.nc` | 4 358 (126 CRLF lines) | `d87d66ec40a63aefe3bf62d5a313a18b625687f36bda088dc798747afbfe9fe2` (byte-exact string; file digest equals `gcode_sha256`) |
| `08-verify_cam_gcode.request.json` | 228 331 | `0f685500b8293a7153a4e526eae9a155dac40350cd29cd20ee2ea08fec6bdcff` |
| `08-verify_cam_gcode.response.json` | 432 | `4c661731c92fed9dcfdaf01790730c41ed29b2daaf500fbdffd17dd1e00c7d75` |
| `controls/exit2-forged-digest.request.json` | 228 331 | `09a9ff354a8edbf1ba2520b50ec851cc485038de4805a8593771d81050b3d186` |
| `controls/exit2-forged-digest.response.json` | 222 | `2a9989d48e0b0716af446e491dedc39e33166fbf5b6e2ef4d6374c0786d4f1f2` |
| `controls/exit3-write-failure.stderr` | 64 | `9e5d7908c6ac2ce21bb2b0b62ceb92292c111f02fa228c41f892f769948552f1` |

Digests carried inside the responses (not of the files, of the bound content):

| Digest | Value |
|---|---|
| `source_sha256` (plate.svg) | `b70417041f210eb8f1da16b405b4cef8b932459ddc0ec19ec1595a26e63cbc2a` |
| `project_sha256` (rungs 4, 5, 7, 8) | `9d8f20cadfffa0d8bea625d68d5218bf5e5877966beab4077fe494d78771a66c` |
| `tables_canonical_sha256` | `a25bdbff7866d76e8751a045c6db23b7d064552d6d2d8cd472808e918af4c72a` |
| `cam_sha256` (rungs 7, 8) | `4e682379b1ff01f1af5e3e259c289079760a65518ef12b13316ef7526f6fb635` |
| `post_profile_sha256` | `e2861b91640a9f740d0b1400448c68bdc9e6ab643f7f38617bbb371f1afa6c2f` |
| `gcode_sha256` (rungs 7, 8) | `d87d66ec40a63aefe3bf62d5a313a18b625687f36bda088dc798747afbfe9fe2` |

The prepare digest (`0da6c418…`) and plan digest (`3f970b1d…`) reproduce byte for
byte on a second invocation of the same saved request, and the research digest
(`11db4be2…`) does too. Repeat runs are not just logically equal; they are
byte-identical.

## Machine-output boundary

Every response in this ladder was checked for authority flags:

- `machine_output_enabled` appears **25 times** across the eight rungs — 25 `false`,
  **0** `true`.
- `hardware_qualified` appears once, in `capabilities`, as `false`.
- `controller_qualified` appears in rungs 7 and 8, both `false`.
- No file under `receipts/2026-11-07/` contains `"qualified":true`.

The G-code candidate names its own limits: historical WAM1.6 reference syntax only,
current client2.4.0 and installed firmware unqualified, centre lines only, full
declared cutting area used for header extents, three 1 s dwells that are historical
samples rather than calibrated pressure-release behaviour, f64 roundtrip preserved
with controller precision unqualified, and job time excluding rapid/acceleration/
hydraulic timing.

Nothing here is a controller qualification, a machine-readiness claim, or a
statement that this G-code is safe to send to hardware.

## Negative controls

These are **not** ladder rungs. They were run to observe the two failure exit codes
the contract describes, in this build.

### Exit 2 — structured error, output channel intact

Forging the candidate's own `gcode_sha256` and re-verifying:

```sh
jq -n --slurpfile p "$R/04-plan_cam.response.json" --slurpfile g "$R/07-research_cam_gcode.response.json" \
  '{schema:"reversal.openwam-request.v1",request:{operation:"verify_cam_gcode",artifact:$p[0].result.artifact,candidate:($g[0].result.candidate | .gcode_sha256="0000000000000000000000000000000000000000000000000000000000000000")}}' \
  > "$C/exit2-forged-digest.request.json"
./target/debug/openwam request "$C/exit2-forged-digest.request.json" > "$C/exit2-forged-digest.response.json"
```

Observed exit **2**, stdout still one valid JSON document:

```json
{"error":{"code":"invalid_candidate","message":"CAM candidate differs from independently rebuilt source/intent/table/postprocessor"},"machine_output_enabled":false,"schema":"reversal.openwam-response.v1","status":"error"}
```

The tamper was rejected on the digest, and `machine_output_enabled` stayed false in
the error response. Verification is not a rubber stamp on retained output.

### Exit 3 — output channel failure

```sh
./target/debug/openwam request "$R/05-verify_cam.request.json" > /dev/full 2> "$C/exit3-write-failure.stderr"
```

Observed exit **3**, stderr `writing OpenWAM response: No space left on device (os error 28)`.
The failure is reported on stderr, not disguised as a successful response. A usage
error (`./target/debug/openwam --help`) also exits **2** with `code: "usage"`, so exit
2 covers request shape as the contract says.

## What verified

- The documented operation sequence in [openwam-api.md](openwam-api.md) executes in
  this checkout against the crate's own `plate.svg`, with every request and response
  retained.
- Independent recomputation agrees at every verification rung: `verify` (project,
  `operation_count: 125`), `verify_cam` (2 paths, 90 operations),
  `verify_cam_gcode` (regenerated code digest equals the retained candidate).
- The plan is internally consistent with its own inputs: 2 tabs on the outline
  produce exactly 3 cut spans and 2 uncut intervals; 4 spans produce 4 pierces,
  4 rapids and 4 stops. Those 90 plan operations appear in the research export as
  78 `G1` moves and 4 `G0` rapids; the pierce/stop sequences become 4×(M3, M8, M5,
  M9) with 16 `G4` dwells totalling 36.0 s, plus the historical header (`G90`,
  `G21`, `M1403`–`M1413`) and the closing `M1404`, for 125 commands on 126 lines
  (line 1 is a comment).
- Deterministic repetition: identical saved requests give byte-identical responses.
- The 125-operation plate count matches the number [openwam.md](openwam.md) reports
  for the retained plate demonstration, which is a consistency check across
  documents rather than a re-run of that older receipt.
- Authority stays disabled in success **and** failure paths.

## What did not verify, and was not attempted

- No `simulate_cam`, `verify_cam_simulation`, `simulate`, `verify_simulation`,
  `simulate_cam_gcode`, `verify_cam_gcode_simulation`, `export_cam_linuxcnc`,
  `verify_cam_linuxcnc`, `inspect_gcode`, `compare_gcode` or `compare_research_gcode`
  rung was run. The ladder in this receipt stops at CAM verification and research
  export, so those links remain documented but unexercised here.
- `wam24_pro_cam` / `wam24_pro_presegmented` were not called. The documented
  rejection of Pro for tabbed multi-span CAM was therefore neither confirmed nor
  contradicted here.
- The preview was not visually inspected. The claim attached to it is structural,
  and was counted in the saved SVG: 16 542 characters of content, a 460×305 mm
  white frame, 3 grey source-boundary polylines (including the uncut annotation),
  78 cyan cut moves, dashed grey rapid segments and 4 orange pierce circles at
  `r=0.45`. Whether it *looks* right to a human is unverified. Note the preview
  draws three rapid segments for the plan's four rapids, so the drawing is not a
  one-glyph-per-plan-operation rendering; the counts above are what the file
  contains, not what an operation count predicts.
- No controller, firmware, machine or material behaviour was tested. Feed 108 mm/min,
  pierce 6 s, kerf 1.1 mm and 36 s of dwell are catalogue and historical values, not
  measurements.
- No hydraulic, corner-quality, tab-strength, acceleration or stock-mechanics model
  was validated.

### The prior `/tmp` lead does not belong to this ladder

The earlier run left two files. Neither is evidence for this receipt:

| File | mtime | SHA-256 | What it is |
|---|---|---|---|
| `/tmp/openwam-preview.svg` | 2026-11-01 21:58 | `371bb366da2c395c423e8c9166703737843b0a7d437f0eeef6d7074635d9d943` | A `preview_cam` of the **cam-panel** fixture — its sub-titles are `svg-contour-0`, `openings/subpath-0`, `openings/subpath-1`, `round-opening`, `annotation` |
| `/tmp/openwam-preview.png` | 2026-11-07 14:07 | — | A raster of that same panel preview; not regenerated or inspected here |

The plate preview produced by this ladder is `2a2bcd48…` (16 543 bytes), so the
`/tmp/openwam-preview.svg` file is a different fixture from a different day. The
timestamps do not even agree with each other (SVG 2026-11-01, PNG 2026-11-07).
Treating it as the plate result would have been wrong, which is why this receipt
was regenerated from `crates/sketch-intake/examples/plate.svg` instead.

## Documentation cross-check

While assembling this receipt, three receipt files referenced by the API docs were
found **absent** from the tree at `c144ef2` (confirmed with `ls`):

- `wazer/openwam-api-demonstration.json` — linked from [openwam-api.md](openwam-api.md)
  ("The CLI demonstration receipt records retained request/response hashes … all 125
  plate operations in 207,158 virtual ms").
- `wazer/openwam-demonstration.json` — linked from [openwam.md](openwam.md).
- `wazer/openwam-ui-demonstration.json` — linked from [openwam.md](openwam.md).

Those three claims are currently unopenable from the repository: the prose asserts
a receipt that is not there. This file does not retro-fill them; it replaces the
claim with an artifact set that is present, dated, and hashed. The remaining
`*demonstration.json` files (`cam-loop`, `independent-cam`, `pro-cam`,
`linuxcnc-waterjet`, `gcode-shutdown`, `gcode-simulation`, `shutdown-replay`) do
exist.

## Scope statement

Software consistency on one fixture, in one build, offline. Verified: the typed CLI
ladder executes, retains its inputs and outputs, recomputes its own artifacts, and
keeps machine authority disabled. Not verified and not claimed: cut quality,
hydraulic timing, tab strength, controller or firmware compatibility, WAM parity, or
that any of this is ready to drive a WAZER.
