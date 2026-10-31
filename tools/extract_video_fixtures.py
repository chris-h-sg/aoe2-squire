"""One-time extraction of vision test fixtures from the test_bench videos.

For every frame listed by `frame_idx` in the *_expected_*.csv files (plus the middle frame
of each pop_color range in extract-housed-frames.csv), saves the frame as a PNG with
everything but the resource panel and the anchor search band blacked out, and writes the expected readings to
test_bench/video_frames/expected_values.json (same format as test_bench/expected_values.json).

The frames keep their full size so the anchor search (fractions of the frame size)
behaves exactly as on the full frame; the black area costs almost nothing in PNG.

Usage:
    python tools/extract_video_fixtures.py [--verify target/release/aoe2-squire.exe]

--verify also saves the unmasked frames to a temp dir and checks that the binary reads
the same values from masked and unmasked frames.
"""

import argparse
import csv
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import cv2

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "test_bench"
OUT_DIR = BENCH / "video_frames"

# Regions kept (fractions of width, height); everything else is blacked out.
# - Resource panel: the ui_map.json elements end at x=708, y=63 px at UI scale 1.0.
# - Anchor: searched at x 50-95% of the width, y 1.5-3.5% of the height (+9 px density box),
#   see ANCHOR_* in src/constants.rs and src/pipeline/anchor.rs. The rightmost red pixel
#   wins and sits 263 px * UI scale from the right edge, so only the right quarter is kept.
KEEP_REGIONS = [
    ((0.0, 0.4), (0.0, 0.07)),
    ((0.75, 1.0), (0.0, 0.05)),
]

# expected CSV column -> (category, sub_key) as returned by pipeline::process_frame
COLUMNS = {
    "wood_total": ("wood", "total"),
    "wood_vils": ("wood", "vils"),
    "food_total": ("food", "total"),
    "food_vils": ("food", "vils"),
    "gold_total": ("gold", "total"),
    "gold_vils": ("gold", "vils"),
    "stone_total": ("stone", "total"),
    "stone_vils": ("stone", "vils"),
    "population_total": ("population", "total"),
    "population_housing": ("population", "housing"),
    "population_vils": ("population", "vils"),
    "idle_vils": ("idle", "vils"),
    "pop_color": ("population", "color"),
    # "housing" (normal/housed/queued) is interpolated over time, not read per frame.
}

EXPECTED_CSVS = [
    ("extract.mp4", "extract_expected_1s.csv"),
    ("extract2.mp4", "extract2_expected_1s.csv"),
    ("extract-housed.mp4", "extract-housed_expected_1s.csv"),
    ("extract-housed.mp4", "extract-housed_expected_0.5s.csv"),
]
POP_COLOR_RANGES = ("extract-housed.mp4", "extract-housed-frames.csv")


def set_value(values, key, value):
    cat, sub = COLUMNS[key]
    values.setdefault(cat, {})[sub] = value


def collect_expected():
    """Returns {video: {frame_idx: {category: {sub_key: value}}}}."""
    wanted = {}
    for video, csv_name in EXPECTED_CSVS:
        with open(BENCH / csv_name, newline="") as f:
            for row in csv.DictReader(f):
                if not row.get("frame_idx"):
                    continue
                values = {}
                for key in COLUMNS:
                    set_value(values, key, row[key].strip())
                frames = wanted.setdefault(video, {})
                idx = int(row["frame_idx"])
                if idx in frames and frames[idx] != values:
                    sys.exit(f"{video} frame {idx}: conflicting expected values in {csv_name}")
                frames[idx] = values

    # Only pop_color is labelled in the range file. Its frame numbers are 1-based (the
    # pipeline's colour transitions on the video land exactly one frame before each range
    # start); take the middle of each range.
    video, csv_name = POP_COLOR_RANGES
    with open(BENCH / csv_name, newline="") as f:
        for row in csv.DictReader(f):
            idx = (int(row["start"]) + int(row["end"])) // 2 - 1
            frames = wanted.setdefault(video, {})
            if idx in frames:
                if frames[idx]["population"]["color"] != row["pop_color"]:
                    sys.exit(f"{video} frame {idx}: pop_color conflict in {csv_name}")
            else:
                frames[idx] = {"population": {"color": row["pop_color"]}}
    return wanted


def read_frames(video_path, indices):
    """Decodes the video sequentially (seeking is not frame-exact) and yields wanted frames."""
    cap = cv2.VideoCapture(str(video_path))
    if not cap.isOpened():
        sys.exit(f"cannot open {video_path}")
    remaining = set(indices)
    idx = 0
    while remaining:
        ok, frame = cap.read()
        if not ok:
            sys.exit(f"{video_path.name}: frames {sorted(remaining)} not found")
        if idx in remaining:
            remaining.discard(idx)
            yield idx, frame
        idx += 1
    cap.release()


def mask(frame):
    h, w = frame.shape[:2]
    masked = frame * 0
    for (x0, x1), (y0, y1) in KEEP_REGIONS:
        xs = slice(int(x0 * w), int(x1 * w))
        ys = slice(int(y0 * h), int(y1 * h))
        masked[ys, xs] = frame[ys, xs]
    return masked


def read_with_binary(binary, image):
    out = subprocess.run([str(binary), str(image)], capture_output=True, text=True, check=True)
    lines = [l.strip() for l in out.stdout.splitlines() if l.startswith("  ")]
    return [l for l in lines if not l.startswith("meta_")] or ["<no anchor>"]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--verify", type=Path, help="aoe2-squire binary used to compare readings")
    args = parser.parse_args()

    wanted = collect_expected()
    OUT_DIR.mkdir(exist_ok=True)
    manifest_path = OUT_DIR / "expected_values.json"
    # known_failures are maintained by hand in the manifest; keep them across regeneration.
    previous = {}
    if manifest_path.exists():
        previous = json.loads(manifest_path.read_text(encoding="utf-8"))["images"]
    for old in OUT_DIR.glob("*.png"):
        old.unlink()
    full_dir = Path(tempfile.mkdtemp(prefix="squire_full_frames_")) if args.verify else None

    images = {}
    mismatches = []
    for video, frames in sorted(wanted.items()):
        stem = Path(video).stem
        for idx, frame in read_frames(BENCH / video, frames):
            name = f"{stem}_f{idx:04d}.png"
            cv2.imwrite(str(OUT_DIR / name), mask(frame), [cv2.IMWRITE_PNG_COMPRESSION, 9])
            images[name] = {"source": f"{video} frame {idx}", "values": frames[idx]}
            if previous.get(name, {}).get("known_failures"):
                images[name]["known_failures"] = previous[name]["known_failures"]
            if full_dir:
                cv2.imwrite(str(full_dir / name), frame)
                masked_read = read_with_binary(args.verify, OUT_DIR / name)
                full_read = read_with_binary(args.verify, full_dir / name)
                if masked_read != full_read:
                    mismatches.append(f"{name}: full {full_read} vs masked {masked_read}")

    manifest = {
        "_comment": "Generated by tools/extract_video_fixtures.py. Frames from the test_bench "
        "videos, blacked out except for the resource panel and the anchor search band.",
        "images": dict(sorted(images.items())),
    }
    with open(manifest_path, "w", newline="\n") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    size = sum(p.stat().st_size for p in OUT_DIR.glob("*.png"))
    print(f"{len(images)} frames, {size / 1e6:.2f} MB in {OUT_DIR.relative_to(ROOT)}")
    if args.verify:
        print(f"verified masked vs full frames: {len(mismatches)} mismatches")
        for m in mismatches:
            print("  " + m)
        sys.exit(1 if mismatches else 0)


if __name__ == "__main__":
    main()
