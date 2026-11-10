"""Feed a deterministic running route into the emulator GNSS at 1 Hz.

argv: adb serial log_path stop_file [timeline t0]
Without a timeline: an ellipse around Berlin Tiergarten (≈2.55 km lap) at a
constant 3 m/s from now until `stop_file` exists; the route depends only on the
fix index, so every run feeds the same geometry. With a timeline (soak.py
route: "t lon lat alt knots state cum_m" rows): row t is fed at host time t0 + t,
until the rows end or `stop_file` exists.
Each log line: "<t>\t<host epoch>\t<lon>\t<lat>\t<state>\tok|FAIL <adb output>".
"""
import math
import os
import subprocess
import sys
import time

LAT0, LON0, ALT = 52.5145, 13.3500, 35.0
A, B = 500.0, 300.0  # ellipse half-axes, m
SPEED = 3.0
M_LAT = 111_195.0
M_LON = M_LAT * math.cos(math.radians(LAT0))
KNOTS = 1.943844


def ellipse():
    lap = math.pi * (3 * (A + B) - math.sqrt((3 * A + B) * (A + 3 * B)))
    i = 0
    while True:
        th = 2 * math.pi * (i * SPEED % lap) / lap
        yield (i, f"{LON0 + A * math.cos(th) / M_LON:.7f}", f"{LAT0 + B * math.sin(th) / M_LAT:.7f}",
               f"{ALT:.1f}", f"{SPEED * KNOTS:.2f}", "run")
        i += 1


def timeline(path):
    for line in open(path):
        t, lon, lat, alt, kn, st, _ = line.rstrip("\n").split("\t")
        yield int(t), lon, lat, alt, kn, st


def main():
    adb, serial, logp, stop = sys.argv[1:5]
    rows, t0 = (timeline(sys.argv[5]), float(sys.argv[6])) if len(sys.argv) > 6 else (ellipse(), time.time())
    with open(logp, "a", buffering=1) as log:
        for t, lon, lat, alt, kn, st in rows:
            if os.path.exists(stop):
                break
            delay = t0 + t - time.time()
            if delay > 0:
                time.sleep(delay)
            r = subprocess.run([adb, "-s", serial, "emu", "geo", "fix", lon, lat, alt, "8", kn],
                               capture_output=True, text=True, timeout=10)
            ok = r.returncode == 0 and "OK" in r.stdout
            log.write(f"{t}\t{time.time():.2f}\t{lon}\t{lat}\t{st}\t"
                      f"{'ok' if ok else 'FAIL ' + (r.stdout + r.stderr).strip()}\n")


if __name__ == "__main__":
    main()
