# Shared shutdown timer and state guards

The Pro 2.32 image uses the same timer at waterjet object `+0x124` and
the same started flag (`+0x185`, mask `0x20`) for **mode application,
cut completion and pausing**. A completion-to-pause transition cannot safely
be modeled by unconditionally resetting that timer.

This is static research against the original 416,324-byte image, SHA-256
`814d8f0abfc5eb0e3d168efa73e7a7f19a20b55e11883fafb8aa603ec16fa47c`,
mapped at `0x4000`. The [receipt](shared-shutdown-timer.json) records thirteen
Ghidra memory spans compared byte for byte with that file and thirty hashed
captures. This establishes source identity and static observations, not vendor
execution or an installed configuration.

| Path | Direct observation |
| --- | --- |
| State change `0x1f580` | No direct write to `+0x185` or timer `+0x124`. Copies current state `+0x51` to previous state `+0x50`; a requested value below `0x6c` becomes current state, otherwise current state becomes `1`. |
| State override `0x1f582–0x1f590` | If `(word_at_+0x50 & 0x00ff00ff) == 0x00010058`, replace the requested state with `0x5a`. This compares bytes `+0x50` and `+0x52`, excluding current-state byte `+0x51`. |
| Prepare pause `0x20f94` | Calls state change with `0x68`, conditionally calls feed-hold helper `0x1f518`, stops timer `+0x118` and clears **mask `0x10`** at `+0x185`, stops timer `+0x100` and clears mask `0x04` at `+0x186`, switches abrasive off, then calls state change with `0x69`. No direct clear of shared mask `0x20`. |
| Completion `0x20264` | Mask `0x20` clear: set it, reset/start `+0x124`, return. Mask set: compare elapsed with `+0xb0`; on strict threshold exceed, stop timer, clear mask `0x20`, write the remaining outputs off and request `0x65`. |
| Pausing `0x20650` | Same flag/timer protocol. Reason byte `+0x53 == 2` selects threshold `+0xb4`; other reasons select `+0xb0`. A set flag enters comparison without resetting the timer. |
| Mode application `0x1f0bc` | Returns unless mask `0x10` at `+0x186` is set and current state is `0x5b`, `0x5c` or `0x0d`. Its mode `0`/`4` shutdown branch can start/read/stop the same shared timer. Cleanup at `0x1f1b2` clears **both** masks `0x10` and `0x20` at `+0x185` with `AND 0xcf`. |
| Feed hold `0x1f518` | If kernel `+0xb0` mask `0x08` is clear, set it. Only while current state is `0x68` and mode `+0x55` is `1` or `2`, set object `+0x181` mask `0x01` and request mode `0` through `0x1f490`. Its immediate apply-mode call still faces the state guard above. |

Mode application therefore supplies another possible timer epoch before entry
to completion. Starting completion in the existing replay means a fresh,
explicitly assumed epoch; it does not reconstruct that prior mode history.

State change also updates water-level bookkeeping and calls Player helpers
`0x28a38`, `0x28598` and `0x28474`. `0x28598` can invoke a function pointer at
Player `+0x190`; `0x28474` can invoke virtual methods on Player stream objects.
Their targets and possible callbacks into the waterjet module are unresolved.
Absence of a direct timer write in state change is therefore **not proof** that
every reachable transition preserves its timer.

The dispatcher at `0x23520` adds further context. Its entire state dispatch is
guarded by waterjet `+0x182` mask `0x20`. Before selecting the current handler,
it can request `0x64` under the `+0x188` mask `0x40` guard (except current states
`0x64`, `0x65`, `0x48`, `0x49`), applies the guarded mode path, and can request
`0x60` based on `+0x185` mask `0x02` and a float comparison. A replay `poll`
currently represents direct execution of the bounded completion/pause handler,
not this whole dispatcher.

These findings retain the existing rejection of completion-to-pause/abort
inputs. They narrow what remains: resolve Player callback targets, actual
state/reason and dispatcher flag histories, then model timer inheritance under
an explicit, verified transition contract. No extra replay behavior was added
by this research.

## Reproduce the source checks

Run from the repository root with the retained original firmware:

```sh
cargo run -p reversal-sketch-intake --example verify_timer_evidence -- \
  wazer/private/shutdown-v232/firmware-v2.32.bin
```

The verifier checks the full file identity, each source span digest, every
retained capture digest, and each Ghidra memory array against the original file.
Missing captures or mismatched bytes fail immediately. It requires the private
captures; the public receipt alone does not replace them.

For manual instruction review, use GNU `arm-none-eabi-objdump` with `-D -b binary
-m arm -M force-thumb --adjust-vma=0x4000`, selecting the addresses above.
Memory spans use `file_offset = mapped_address - 0x4000`. The existing
[timer comparison receipt](shutdown-timer-boundary.json) establishes the strict
`threshold < elapsed` comparison; this research does not change that boundary.
