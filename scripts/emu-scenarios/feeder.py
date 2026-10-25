"""Feed a deterministic running route into the emulator GNSS at 1 Hz.

argv: adb serial log_path stop_file [speed_mps]
Runs an ellipse around Berlin Tiergarten (≈2.55 km lap) at a constant speed from
t=0 until `stop_file` exists. Each line of log_path: "<i> <host_epoch> <lon> <lat> ok|FAIL".
The route depends only on the fix index, so every run feeds the same geometry.
"""
import math
import os
import subprocess
import sys
import time

adb, serial, logp, stop = sys.argv[1:5]
speed = float(sys.argv[5]) if len(sys.argv) > 5 else 3.0
LAT0, LON0, ALT = 52.5145, 13.3500, 35.0
A, B = 500.0, 300.0
M_LAT = 111_195.0
M_LON = M_LAT * math.cos(math.radians(LAT0))
lap = math.pi * (3 * (A + B) - math.sqrt((3 * A + B) * (A + 3 * B)))

t0 = time.time()
i = 0
with open(logp, "a", buffering=1) as log:
    while not os.path.exists(stop):
        th = 2 * math.pi * (i * speed % lap) / lap
        lat = LAT0 + B * math.sin(th) / M_LAT
        lon = LON0 + A * math.cos(th) / M_LON
        r = subprocess.run([adb, "-s", serial, "emu", "geo", "fix", f"{lon:.7f}", f"{lat:.7f}",
                            f"{ALT:.1f}", "8", f"{speed * 1.943844:.2f}"],
                           capture_output=True, text=True, timeout=10)
        ok = r.returncode == 0 and "OK" in r.stdout
        log.write(f"{i} {time.time():.2f} {lon:.7f} {lat:.7f} {'ok' if ok else 'FAIL'}\n")
        i += 1
        d = t0 + i - time.time()
        if d > 0:
            time.sleep(d)
