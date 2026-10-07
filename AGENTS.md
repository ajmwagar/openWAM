# Agent instructions

Rust owns first-party implementation. Keep geometry with Transmog and process
compilation with Swarf; `vendor/` contains pinned owner snapshots, not independent
forks. Preserve dependency licenses and source provenance when refreshing them.

Use typed API boundaries, deterministic workflows and independently recomputable
artifacts. Keep original drawing source immutable. Explicitly reject unsupported
geometry; do not silently repair it or turn on machine output.

Iterate with `cargo check`; run relevant tests, strict Clippy and formatting before
committing completed work. The README gives the workspace commands. Local commits
are allowed for verified work. Remote publication needs authorization appropriate
to the source being published, including private owner dependencies.

Use Beads for durable task tracking. The extraction is tracked as `reversal-dqw`
in the adjacent Reversal workspace until standalone tracking is initialized.
Do not create markdown task lists or MEMORY.md. Keep changes focused, preserve
unrelated work and clearly report validation and remaining limits.
