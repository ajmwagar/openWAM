# Authored CAM to WAZER Pro research G-code

`research_cam_gcode` accepts a verified CAM artifact and the explicit
`wam24_pro_cam` profile. SVG/DXF source, project/material-table selection,
placement, kerf, contour order, tabs and leads are independently rebuilt before
Swarf exports the derived Rapid/Pierce/Cut/Stop operations. Every open span is
kept open. No fabricated closure or extra endpoint is added.

```json
{"schema":"reversal.openwam-request.v1","request":{"operation":"research_cam_gcode","artifact":{},"profile":"wam24_pro_cam"}}
```

Replace `artifact` with the retained result of `plan_cam`, then run
`openwam request request.json`. Verify using `verify_cam_gcode` with the original
artifact and returned candidate. The verifier rebuilds source, intent, tables,
profile and complete code, including typed commands and authority flags.
Candidates expose SHA-256 bindings for the selected post profile and G-code.
Use `simulate_cam_gcode` and `verify_cam_gcode_simulation` to replay and verify
the emitted commands.

The profile requires a Pro material recipe. It writes the observed 2.4.0 header,
20-character material/thickness fields, `M3/M8` pierce sequence and `M9/M5`
shutdown sequence. The pre-abrasive-stop dwell is
`max(1, floor(0.15 * pierce_seconds))`, followed by two one-second dwells.
The same owner helper serves the existing Pro service parity profile.

Coordinates and feeds preserve the authored CAM values rather than using WAM's
two-decimal formatter or corner-feed arrays. Displayed time is nominal cut and
pierce time plus emitted shutdown dwells, rounded upward to whole seconds.
Vendor displayed time, feed-factor indexing, repeated final endpoints and
upstream SVG/offset/tab processing are deliberately outside this profile's
parity claim. The separately named `wam24_pro_presegmented` profile retains its
single closed-path service differential behavior and cannot export CAM spans.

The panel fixture has six separately pierced spans and 90 cut moves, including
holes, two uncut tab gaps and waste-side leads. The regression tests compare
every exported motion/feed against independently rebuilt CAM operations,
replay the exported commands, and reject changed source/table/profile/code
bindings and forged authority. All machine/controller/hardware flags remain
false. Installed firmware compatibility, hydraulic timing and physical cutting
are unqualified; this does not enable machine transfer.

The [retained demonstration](pro-cam-demonstration.json) binds the actual CLI
export/replay verification to the [research panel](fixtures/pro-cam-panel.nc).
The original Node service oracle was executed again and returned bytes identical
to its pinned response. Its shutdown probes agree with the shared dwell policy.
For the nominal square, this authored profile emits four F108 cuts and displays
76 seconds; the original supplied-path service emits five cuts with effective
feeds 108/70.2/54 and displays 98 seconds. Those differences are explicit and
expected; source-import precision and canonical recipe metadata also differ.
