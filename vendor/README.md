# Pinned source dependencies

These minimal source snapshots make the initial standalone checkout reproducible
while the exact owner commits are not available through public release channels.
This is a deliberate packaging exception to avoiding source duplication: no
geometry or process algorithm is maintained independently here. The dependency
owners remain the single source of truth and receive implementation changes.

| Owner | Included crate | Revision | License |
|---|---|---|---|
| FuturePresentLabs/transmog | transmog-core | 5f4f584 | MIT OR Apache-2.0 |
| FuturePresentLabs/swarf | swarf-waterjet | d555b24 | AGPL-3.0 |

Only committed crate files are copied. Transmog's original workspace package,
dependency and lint settings are reproduced in the root Cargo workspace so its
unchanged manifest resolves. Swarf's relative dependency path still points at
the neighboring Transmog snapshot. Source files are byte-identical to their
recorded owner revisions. No private repository history or unrelated crates
are included.

Refresh from an explicitly selected verified owner commit. Preserve its license
and provenance, run the workspace checks, and record the new revision and hashes
in `sources.json`. Prefer replacing a snapshot with a pinned owner release when
one is available; do not grow an application-owned fork.
