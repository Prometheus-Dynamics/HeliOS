#!/usr/bin/env python3
"""Mock camera data for the UI, from the robot video (recording_7d7ca24f).

Writes static/mock/robot/ : JPEG frames (640x400), detections.json with tag
outlines/corners per frame, and poses.json with camera-frame tag poses from
solvePnP (approximate intrinsics, 0.1651 m FRC tags). Not shipped; dev only.

usage: gen-mock.py <raw gray 1280x800> <detections.jsonl from the CM5 run> [frames] [stride]
"""
import json, os, sys
import cv2, numpy as np

raw_path, det_path = sys.argv[1], sys.argv[2]
count = int(sys.argv[3]) if len(sys.argv) > 3 else 480
stride = int(sys.argv[4]) if len(sys.argv) > 4 else 2
out = os.path.join(os.path.dirname(__file__), "..", "static", "mock", "robot")
os.makedirs(out, exist_ok=True)
W, H = 1280, 800
raw = np.memmap(raw_path, dtype=np.uint8, mode="r").reshape(-1, H, W)
dets = [json.loads(l) for l in open(det_path)]
K = np.array([[700.0, 0, W / 2], [0, 700.0, H / 2], [0, 0, 1]])
s = 0.1651 / 2
obj = np.array([[-s, s, 0], [s, s, 0], [s, -s, 0], [-s, -s, 0]], np.float32)
frames, poses = [], []
for k in range(count):
    i = (k * stride) % raw.shape[0]
    img = cv2.resize(np.array(raw[i]), (640, 400), interpolation=cv2.INTER_AREA)
    cv2.imwrite(os.path.join(out, f"{k:04d}.jpg"), img, [cv2.IMWRITE_JPEG_QUALITY, 72])
    markers = []
    pose_list = []
    for m in dets[i]["markers"]:
        c = np.array(m["corners"], np.float32)
        markers.append({"id": m["id"], "corners": (c / 2).round(2).tolist()})
        ok, rvec, tvec = cv2.solvePnP(obj, c, K, None, flags=cv2.SOLVEPNP_IPPE_SQUARE)
        if ok:
            pose_list.append({"id": m["id"], "t": tvec.ravel().round(3).tolist(), "r": rvec.ravel().round(4).tolist()})
    frames.append({"source_frame": i, "markers": markers})
    poses.append(pose_list)
json.dump({"width": 640, "height": 400, "fps": 16.6, "frames": frames}, open(os.path.join(out, "detections.json"), "w"))
json.dump({"intrinsics": {"fx": 700, "fy": 700, "cx": 640, "cy": 400, "note": "approximate"}, "tag_size_m": 0.1651, "frames": poses}, open(os.path.join(out, "poses.json"), "w"))
print(f"wrote {count} frames to {out}")
