# Contributing

OpenWAM is licensed under AGPL-3.0-or-later. All first-party contributions require
acceptance of [the Contributor License Agreement](CLA.md), matching PedalKernel's
copyright-assignment terms in favor of Avery Wagar. The agreement includes a
license back to contributors and a commitment to keep an AGPL version public.
Contributors acting for an employer must have authority to enter the agreement.

Read the full agreement before submitting a contribution. CLA section 16 describes
electronic acceptance by submission; a signed agreement may also be provided.
An automated signing service must be configured separately before claiming its
status checks are enforced. The repository does not imply that an unsigned
contributor has completed a third-party signing flow.

Keep changes focused and include relevant tests. Run the development checks in
the README. Rust owns first-party implementation. Geometry and waterjet process
changes belong in their dependency owners rather than duplicate implementations
in the application. Clearly identify third-party sources and their licenses;
this CLA does not assign copyright you do not own.

Preserve original drawing source, explicit units, retained intent and artifact
verification. Never silently repair geometry or enable machine output as part
of an unrelated change. Report the supported scope and evidence for new behavior.
