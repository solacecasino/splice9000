"""Makes two sample clips with ffmpeg and a session that opens them, for
trying splice9000 (and for CI's screenshots).

    python3 scripts/sample.py <folder>

Then start splice9000 with SPLICE9000_CONFIG_DIR=<folder>/config.
"""
import json
import os
import subprocess
import sys

out = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "sample")
media = os.path.join(out, "media")
config = os.path.join(out, "config")
os.makedirs(media, exist_ok=True)
os.makedirs(config, exist_ok=True)

clips = [
    # name, size, fps, seconds, sound
    ("bars.mp4", (1280, 720), 30, 6, True),
    ("fractal.mp4", (640, 360), 25, 4, False),
]
entries = []
for name, (w, h), fps, secs, sound in clips:
    path = os.path.join(media, name)
    source = f"testsrc2=s={w}x{h}:r={fps}:d={secs}" if sound else f"mandelbrot=s={w}x{h}:r={fps}"
    cmd = ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", source]
    if sound:
        cmd += ["-f", "lavfi", "-i", f"sine=f=330:d={secs}", "-shortest"]
    cmd += ["-t", str(secs), "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", path]
    subprocess.run(cmd, check=True)
    entries.append({"path": path, "duration": float(secs), "width": w, "height": h, "fps": float(fps), "audio": sound})

session = {
    "media": entries,
    "clips": [
        {"media": 0, "src_in": 1.0, "src_out": 4.0},
        {"media": 1, "src_in": 0.0, "src_out": 3.0},
        {"media": 0, "src_in": 4.0, "src_out": 6.0},
    ],
    "playhead": 1.5,
}
with open(os.path.join(config, "session.json"), "w") as f:
    json.dump(session, f)
print(config)
