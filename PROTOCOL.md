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

- `firmware/zero-ws2812` (single WS2812): breathing blue for `B`, solid amber/red
  for `A`/`R`, dim white on boot before the first command, blinking red on watchdog.
- `firmware/tdisplay-s3` (170x320 LCD): colour plus on-screen text, and room to show
  more than the six states below allow.

## Planned extension

A `T<text>` command (text to display) is reserved for the richer displays (LCD,
matrix). `netled` does not send it yet. Boards should ignore it until it is defined
here, per the "unknown lines are ignored" rule above.
