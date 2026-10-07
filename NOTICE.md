# Licensing and source provenance

OpenWAM application code is Copyright (C) 2026 Avery Wagar and distributed under
AGPL-3.0-or-later. The root `LICENSE` contains the complete standard GNU Affero
General Public License version 3 text. No revenue-based hardware restriction is
added. The Contributor License Agreement concerns first-party contributions; it
does not assign ownership of third-party dependencies.

The application was extracted from Reversal's `crates/sketch-intake` at
`dff7cb3`. Its existing Rust names and JSON protocol identities are retained.
The historical application package was offered under `MIT OR Apache-2.0`;
the new first-party distribution uses the maintainer's requested AGPL license.
Historical permissive grants are not revoked. Corresponding standard license
texts are retained in `LICENSES/`.

The `reversal-waterjet-control` crate is an independently written source
dependency from the same Reversal revision. Its `GPL-3.0-or-later` license and
`COPYING` remain unchanged. Its synthetic simulator is not a manufacturer
firmware implementation or a calibrated hydraulic model.

Transmog owns geometry and Swarf owns waterjet process compilation. Their source
licenses remain `MIT OR Apache-2.0` and `AGPL-3.0`, respectively; the application
license does not replace them. Changes belong in those owners. Distribution of
dependency source must preserve its owner provenance and license notices.

`wazer/cutting-tables.json` contains normalized factual material/specification
data from public manufacturer pages. Source URLs and hashes remain in
`wazer/cutting-data-acquisition.json`. Documentation and evidence files may cite
research artifacts retained elsewhere; they do not imply those artifacts are
distributed here. No manufacturer firmware, captured JavaScript, private account
material or support-document binaries are included in this repository.

The CLA was copied from `ajmwagar/pedalkernel/CLA.md`, Git blob
`4ffac6a4fcfe7aab6fa6b4bac910f28d5e711345`. One stray terminal escape sequence
in section 8 was removed; the agreement's wording is unchanged.
