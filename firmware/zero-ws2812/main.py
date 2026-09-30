# Internet status light for the router (Waveshare ESP32-S3-Zero).
# The router writes one command per line over USB serial:
#   B = online (breathing blue), R = offline (red), A = degraded/reconnecting (amber),
#   G = green, W = white, O = off.
# No command for TIMEOUT_MS -> blink red (router hung or service stopped).
import sys, select, time, math, machine, neopixel

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
BREATHE_MS = 4000  # one full dark -> bright -> dark cycle while online
TICK_MS = 20

current = None


def show(color):
    global current
    if color != current:
        current = color
        np[0] = color
        np.write()


def breathe(now):
    # Raised cosine 0..1, squared so the fade looks even to the eye
    # (LED output is linear, perception is not).
    phase = time.ticks_diff(now, breathe_start) % BREATHE_MS
    v = (1 - math.cos(2 * math.pi * phase / BREATHE_MS)) / 2
    show((0, 0, round(LEVEL * v * v)))


show((LEVEL // 4, LEVEL // 4, LEVEL // 4))  # dim white: booted, waiting for router
state = None
last = time.ticks_ms()
breathe_start = last
blink_last = last
blink = False
buf = ""
poll = select.poll()
poll.register(sys.stdin, select.POLLIN)

while True:
    if poll.poll(TICK_MS):
        ch = sys.stdin.read(1)
        if ch in "\r\n":
            cmd = buf.strip().upper()
            buf = ""
            if cmd in COLORS:
                last = time.ticks_ms()
                if cmd != state:
                    state = cmd
                    breathe_start = last
                if cmd != "B":
                    show(COLORS[cmd])
        else:
            buf = (buf + ch)[-16:]
    now = time.ticks_ms()
    if time.ticks_diff(now, last) > TIMEOUT_MS:
        if time.ticks_diff(now, blink_last) >= 500:
            blink_last = now
            blink = not blink
            show(COLORS["R"] if blink else COLORS["O"])
    elif state == "B":
        breathe(now)
