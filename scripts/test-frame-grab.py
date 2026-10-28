"""Runs the Frame Grab adapter the way splice9000 does, playing splice9000's
side of the protocol, and checks it saves the right frame as a PNG.

    python3 scripts/test-frame-grab.py <path to the frame-grab program>
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

program = os.path.abspath(sys.argv[1])
ffmpeg = shutil.which("ffmpeg") or sys.exit("needs ffmpeg")
ffprobe = shutil.which("ffprobe") or sys.exit("needs ffprobe")
work = tempfile.mkdtemp()
clip = os.path.join(work, "clip.mp4")
subprocess.run([ffmpeg, "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=s=640x360:r=25:d=4",
                "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", clip], check=True)
out = os.path.join(work, "grabbed.png")

p = subprocess.Popen([program], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
def send(m):
    p.stdin.write(json.dumps(m) + "\n")
    p.stdin.flush()
def receive():
    line = p.stdout.readline()
    if not line:
        sys.exit("frame-grab stopped early")
    return json.loads(line)

send({"method": "initialize", "params": {"splice9000": "test", "protocol": 1, "permissions": ["edit.read"], "ffmpeg": ffmpeg}})
send({"method": "command", "params": {"command": "grab"}})
edit = {"playhead": 2.5, "media": [{"path": clip, "duration": 4.0, "width": 640, "height": 360, "fps": 25.0, "audio": False}],
        "clips": [{"media": 0, "src_in": 0.5, "src_out": 4.0, "start": 0.0}]}
asked = []
while True:
    m = receive()
    method = m.get("method")
    asked.append(method)
    if method == "edit.get":
        send({"id": m["id"], "result": edit})
    elif method == "ui.save_dialog":
        assert m["params"]["file_name"] == "clip frame 75.png", m  # 0.5 + 2.5 s at 25 fps
        send({"id": m["id"], "result": out})
    elif method == "notify":
        print("notify:", m["params"]["text"])
        assert m["params"]["text"].startswith("Saved"), m
        break
send({"method": "shutdown"})
p.wait(timeout=10)
assert asked == ["edit.get", "ui.save_dialog", "notify"], asked
size = subprocess.run([ffprobe, "-v", "error", "-of", "csv=p=0", "-show_entries", "stream=width,height", out],
                      capture_output=True, text=True, check=True).stdout.strip()
assert size == "640,360", size
print("Frame Grab saved a 640x360 PNG of frame 75: ok")
