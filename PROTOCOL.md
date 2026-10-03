# Status protocol

The router-side monitor (`router/netled`) decides the internet state and sends it
to a display board over USB CDC serial. This file is the contract between the two
sides. Any display firmware that honours it works with the existing monitor; the
router side does not change per board.

## Transport

- USB CDC serial, default line settings, one command per line.
- The monitor writes a single command letter followed by a newline (`\n` or `\r\n`).
- The board reads one line at a time, trims whitespace, and upper-cases it.
- Unknown lines are ignored. Lines longer than a small bound are truncated, so a
  garbled burst cannot wedge the parser.

## Commands

| Cmd | State | Meaning |
|-----|-------|---------|
| `B` | online | Internet reachable. |
| `A` | degraded | 1-2 failed checks: a blip, or PPPoE reconnecting. |
| `R` | offline | 3+ failed checks (offline ~15 s or more). |
| `G` | green | Free/unused; test or custom. |
| `W` | white | Free/unused; test or custom. |
| `O` | off | Blank the display. |

## Watchdog

If the board receives no recognized command for 60 s, it must show a distinct
"no signal from the router" state (the router hung, or the monitor died). This is
separate from `R`: `R` means the monitor is alive and reports the internet down;
the watchdog means the monitor itself went silent.

## Rendering is per board

The command set is the contract; how a board shows each state is up to its firmware.

- `firmware/zero-s3` (WS2812 plus an LED strip): breathing blue, and a glow
  sweeping along the strip, for `B`; amber for `A`; red flashing 0.5 s on, 0.5 s
  off for `R`; dim white on boot before the first command; a slow red pulse on
  watchdog.
- `firmware/tdisplay-s3` (170x320 LCD): colour plus on-screen text, and room to show
  more than the six states below allow.

## Settings

Settings live on the router in `/etc/status-light.json` (see `settings.schema.json`),
keyed by each board's USB serial number. netled sends a board its settings as
`S` lines when the board connects and whenever the file changes, always starting
with `S reset`:

```
S reset                     restore the built-in defaults; the values below follow
S brightness 40             percent, 0-100
S strands 40                wisp flame: strands, height, sway, speed, glow, width
S sway 60                     (see WispParams)
S strip_leds 60             LED strip: number of LEDs, 0-300
S strip_pattern comet         pattern while online: sweep, comet, converge,
                                breathe, wave, heartbeat, twinkle
S strip_speed 30              speed, 1-100
S strip_width 8               width in LEDs, 1-50
S strip_primary #ff2000       colours, #rrggbb or #rgb: a gradient from the primary
S strip_secondary #ffd000       (head, centre) to the secondary (tail, edges),
S strip_background #000830      over the background (outside the pattern)
S strip_identify 1            show a counting pattern instead of the status
```

A board applies the keys it supports and ignores the rest, so a board without
settings support ignores every `S` line under the "unknown lines are ignored" rule.
`S` lines also count as traffic for the watchdog.

## Planned extension

A `T<text>` command (text to display) is reserved for the richer displays (LCD,
matrix). `netled` does not send it yet. Boards should ignore it until it is defined
here, per the "unknown lines are ignored" rule above.
