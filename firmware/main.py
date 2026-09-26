# Internet status light for the router (Waveshare ESP32-S3-Zero).
# The router writes one command per line over USB serial:
#   B = online (blue), R = offline (red), A = degraded/reconnecting (amber),
#   G = green, W = white, O = off.
# No command for TIMEOUT_MS -> blink red (router hung or service stopped).
import sys, select, time, machine, neopixel

np = neopixel.NeoPixel(machine.Pin(21), 1)
np.ORDER = (0, 1, 2, 3)  # this board's LED takes RGB, not the default GRB

LEVEL = 40  # brightness 0-255; 40 is plenty for a status light
COLORS = {
    "B": (0, 0, LEVEL),
    "R": (LEVEL, 0, 0),
    "A": (LEVEL, LEVEL // 3, 0),
    "G": (0, LEVEL, 0),
    "W": (LEVEL, LEVEL, LEVEL),
    "O": (0, 0, 0),
}
TIMEOUT_MS = 60000


def show(color):
    np[0] = color
    np.write()


show((LEVEL // 4, LEVEL // 4, LEVEL // 4))  # dim white: booted, waiting for router
last = time.ticks_ms()
blink = False
buf = ""
poll = select.poll()
poll.register(sys.stdin, select.POLLIN)

while True:
    if poll.poll(250):
        ch = sys.stdin.read(1)
        if ch in "\r\n":
            cmd = buf.strip().upper()
            buf = ""
            if cmd in COLORS:
                show(COLORS[cmd])
                last = time.ticks_ms()
        else:
            buf = (buf + ch)[-16:]
    if time.ticks_diff(time.ticks_ms(), last) > TIMEOUT_MS:
        blink = not blink
        show(COLORS["R"] if blink else COLORS["O"])
