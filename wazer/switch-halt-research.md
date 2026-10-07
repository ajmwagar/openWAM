# Pro 2.32 Switch and HALT trace

The vendor Switch matchers deliberately reject the waterjet command family.
Embedded Switch assignments for M3/M5 and M8/M9 consequently do not establish
another writer for those commands. HALT follows a separate callback that can
write configured output values before the waterjet module requests its
pause/abort state.

This extends [the shutdown trace](shutdown-research.md). The
[machine-readable receipt](switch-halt-research.json) pins the same official
416,324-byte Pro 2.32 image, mapped at `0x4000`. Original bytes and decompilation
remain in ignored private captures. These are static observations; installed
configuration, electrical outputs and hydraulic response remain unqualified.

## G-code ownership

Both Switch matchers (`0x1bbcc`, on; `0x1bc40`, off) return false for:

| Rejected M codes | Branch in each matcher |
|---|---|
| M3, M5 | Equality tests |
| M8–M9 | Unsigned `code - 8 < 2` |
| M31, M33 | Equality tests |
| M42–M47 | Unsigned `code - 0x2a < 6` |
| M1403–M1408 | Unsigned `code - 0x57b < 6` |

These exclusions precede the configured command comparison. Other configured
M/G commands must also match their letter, number and subcode. Switch G-code
callback `0x1bcb4` drains the conveyor through `0xcb00` before changing a
digital output on a matching command. This does not imply an installed wiring
map or exclude other modules from handling the same event.

## HALT boundary and ordering

Kernel dispatcher `0x05f20` treats event `8` with argument zero as HALT activation;
nonzero clears its halted flag. It visits the stored subscriber vector in order.
The callback table at `0x4a6a8` selects virtual offset `0x2c` for event 8;
the Switch vtable at `0x4c428` resolves that slot to `0x1bb20`.

The initialization path at `0x04584` loads enabled Switch modules through
`0x1cc24` before constructing/loading the waterjet module. Module loading
`0x05f14` immediately invokes virtual offset `8`. Switch entry `0x1cbe0`
registers events `2, 0, 6, 7, 8` through `0x6e28`, which appends to the vectors.
Thus the traced startup path places enabled Switch HALT callbacks before the
waterjet HALT callback. This establishes relative source order, not an observed
live subscriber list or the order of every module.

Switch HALT callback `0x1bb20` returns immediately for a nonzero argument or
`ignore_on_halt` (object `+0x2a`, bit 7). Otherwise:

| Output type | Traced action |
|---|---|
| Digital (`2`) | Connected pin gets logical bit 0 of object `+0x2b`; inversion controls GPIO SET/CLEAR |
| Sigma-delta PWM (`1`) | Calls `0x077bc` with that bit |
| Hardware PWM (`3`) | Calls `0x3e1f8` with zero |

The cached state bit (`+0x2a`, bit 6) is assigned the configured bit. Clearing
HALT performs no write in this Switch callback; it does not restore the previous
output. The kernel subsequently calls position synchronization `0xd7b4` when
HALT is cleared or the conveyor was not idle before dispatch.

Waterjet callback `0x22fa0` conditionally aborts the player and requests state
`0x61` for several current states, as traced previously. That is a deferred state
request; it does not make the Switch write and timed waterjet pause an atomic
shutdown. The waterjet callback's complete interaction with HALT-clear and its
additional guards remains to be modeled separately.

## Config spelling and defaults

Vendor loader `0x1c06c` uses namespace checksum `0x3c94` (`switch`), module ID,
and the following Fletcher16 key checksums:

| Key | Checksum | Destination/default |
|---|---|---|
| `startup_state` | `0x9298` | `+0x2a`, bit 6; default 0 |
| `failsafe_set_to` | `0xde2e` | `+0x2b`, bit 0; numeric default 0 |
| `ignore_on_halt` | `0x91cd` | `+0x2a`, bit 7; default false |

The embedded six-channel examples spell the key **`fail_safe_set_to`**, whose
checksum is **`0xf48d`**. That is a different lookup identifier. Those lines alone
do not set the traced field; the loader's fallback is zero. This mismatch alone
does not demonstrate an unsafe output. Runtime configuration precedence,
aliases elsewhere and the installed SD configuration have not been established.

The same loaded field controls HALT and debugger-mask registration in this
vendor loader. The [pinned upstream Switch source](https://github.com/Smoothieware/Smoothieware/blob/38e2cc083db0e4f768535a9bf2d32cdf104ea980/src/modules/tools/switch/Switch.cpp)
has separate `failsafe_set_to` and `halt_set_to` keys. It is a comparison source,
not an exact vendor source tree; importing its separate HALT setting would change
the traced vendor behavior.

## Watchdog and debugger limit

The Switch loader registers parsed port/pin masks through `0x6e38` (high) or
`0x6e58` (low), selected by that same failsafe bit. These helpers OR the bit into
five-port RAM arrays at `0x100002c8` and `0x100002b4`, with port/pin bounds. The
observed registration branches do not apply the pin inversion bit when choosing
high versus low. Logical HALT writes and physical debugger masks must therefore
remain distinct in any model.

The watchdog IRQ at `0x937c` conditionally updates indicator state, calls
`0x43d8` to clear watchdog-mode bit 2, calls `0x43e8` to feed `0xaa/0x55`, and
reaches `BKPT` at `0x93b2`. This IRQ has no direct call to kernel HALT dispatch.

The [pinned upstream debugger hooks](https://github.com/Smoothieware/Smoothieware/blob/38e2cc083db0e4f768535a9bf2d32cdf104ea980/src/libs/MRI_Hooks.cpp)
save GPIO state, apply high/low masks on entry and restore masked outputs on
exit. The vendor breakpoint-to-hook call chain and actual mask consumers have
**not** been established. Upstream behavior is not evidence that this vendor
watchdog latches all six channels off, restores them, or resets the controller.

## Reproduce and use

The receipt records 16 matching 64-byte function spans, the complete 4,728-byte
Switch region, a 4,096-byte embedded-config region and both dispatch tables.
Each bridge `hex` capture was compared with original firmware at
`mapped address - 0x4000`; capture SHA-256 values detect later drift. Raw Thumb
disassembly independently retains the matcher branches and loader writes.

Use the corrected Ghidra program `firmware-v2.32-base4000.bin` in the existing
Reversal project described in the shutdown runbook. Function creation adds
analysis annotations only; the module entry is `0x1cbe0`, including the initial
flag load before its push instruction.

```sh
shasum -a 256 wazer/private/shutdown-v232/firmware-v2.32.bin
arm-none-eabi-objdump -D -b binary -m arm -M force-thumb --adjust-vma=0x4000 \
  --start-address=0x1bb20 --stop-address=0x1cd98 \
  wazer/private/shutdown-v232/firmware-v2.32.bin
```

The independent Rust sequence model can now distinguish command exclusions,
ordered HALT writes, logical inversion and HALT-clear behavior. Config precedence
and watchdog/debugger execution need further evidence. The existing synthetic
replay remains unchanged and separately identified; no controller adapter,
flashing or machine output was added. Follow-up implementation remains tracked
in `reversal-faq`; this evidence pass is `reversal-faq.1`.
