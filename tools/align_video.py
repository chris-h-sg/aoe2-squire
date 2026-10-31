"""Finds where a telemetry CSV sits in a video of the same game, and converts CSV times to
video times.

Works for videos recorded with --record-video (telemetry_<start>.mkv) and for any other
recording of the screen (e.g. Steam's recorder), which can start before the match and has
no sidecar. The resource panel is read from video frames with the aoe2-squire binary in
image mode and matched against the CSV rows:

1. Coarse: one frame every --step seconds over the whole video. Each reading votes for the
   offsets at which the CSV shows the same food/wood/gold/stone/population; the most voted
   offset wins.
2. Exact: where the panel changes between two samples, the change is narrowed down to one
   video frame, and the CSV rows before and after the same change bound the offset. The
   bounds from all changes in a part of the video are intersected. Doing this separately
   for --segments parts of the video shows whether the offset drifts.

offset_ms = CSV timestamp_ms - video time in ms, so a CSV row (or a sanity report "rw"
timestamp) T is at video time T - offset_ms.

Usage:
    python tools/align_video.py <video> <telemetry.csv> [--at T ...]
    python tools/align_video.py <video> <telemetry.csv> --offset-ms <ms> --at T ...

--at accepts a CSV timestamp_ms / rw value (e.g. 1778156207466), or a time since the first
CSV row as seconds or [h:]mm:ss[.fff] (e.g. 12:34.5).
"""

import argparse
import csv
import json
import subprocess
import sys
import tempfile
from bisect import bisect_right
from collections import Counter
from pathlib import Path

import cv2

from extract_video_fixtures import KEEP_REGIONS

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BINARY = ROOT / "target" / "release" / "aoe2-squire.exe"

# CSV column -> field printed by the binary in image mode.
FIELDS = {
    "food_total": "food_total",
    "food_vils": "food_vils",
    "wood_total": "wood_total",
    "wood_vils": "wood_vils",
    "gold_total": "gold_total",
    "gold_vils": "gold_vils",
    "stone_total": "stone_total",
    "stone_vils": "stone_vils",
    "pop_curr": "population_total",
    "pop_max": "population_housing",
    "pop_vils": "population_vils",
    "idle_vils": "idle_vils",
}
# Columns compared to decide whether a frame and a row show the same panel. The *_vils
# counts are left out: they change rarely and are the most often misread.
KEY = ("food_total", "wood_total", "gold_total", "stone_total", "pop_curr")
BATCH = 50  # images per binary run (6 MB each at 1080p while they exist)
VOTE_BIN_MS = 500


def fmt_time(ms):
    sign = "-" if ms < 0 else ""
    ms = abs(round(ms))
    h, rest = divmod(ms, 3_600_000)
    m, rest = divmod(rest, 60_000)
    return f"{sign}{h}:{m:02d}:{rest / 1000:06.3f}"


def parse_at(value, csv_start_ms):
    """CSV timestamp_ms, or a time since the first CSV row, to a CSV timestamp_ms."""
    if value.isdigit() and int(value) > 10**11:
        return int(value)
    secs = 0.0
    for part in value.split(":"):
        secs = secs * 60 + float(part)
    return csv_start_ms + round(secs * 1000)


def load_csv(path):
    times, keys = [], []
    with open(path, newline="") as f:
        for row in csv.DictReader(f):
            times.append(int(row["timestamp_ms"]))
            keys.append(tuple(row[c] for c in KEY))
    if not times:
        sys.exit(f"{path} has no rows")
    return times, keys


class Video:
    def __init__(self, path):
        self.cap = cv2.VideoCapture(str(path))
        if not self.cap.isOpened():
            sys.exit(f"cannot open {path}")
        fps = self.cap.get(cv2.CAP_PROP_FPS) or 30
        self.frame_ms = 1000 / fps
        self.duration_ms = self.cap.get(cv2.CAP_PROP_FRAME_COUNT) * self.frame_ms

    def sample(self, step_ms):
        """Yields (time_ms, frame) about every step_ms, reading the video front to back."""
        self.cap.set(cv2.CAP_PROP_POS_FRAMES, 0)
        next_ms = 0.0
        while self.cap.grab():
            t = self.cap.get(cv2.CAP_PROP_POS_MSEC)
            if t + self.frame_ms / 2 >= next_ms:
                ok, frame = self.cap.retrieve()
                if ok:
                    yield t, frame
                next_ms = t + step_ms

    def frame_at(self, target_ms):
        """The first frame at or after target_ms, with its exact time. Seeks before the
        target and reads forward, since seeking alone can land past it."""
        back = 1000
        while True:
            self.cap.set(cv2.CAP_PROP_POS_MSEC, max(0, target_ms - back))
            if not self.cap.grab():
                return None
            t = self.cap.get(cv2.CAP_PROP_POS_MSEC)
            if t <= target_ms or target_ms - back <= 0:
                break
            back *= 2
        while t + self.frame_ms / 2 < target_ms:
            if not self.cap.grab():
                return None
            t = self.cap.get(cv2.CAP_PROP_POS_MSEC)
        ok, frame = self.cap.retrieve()
        return (t, frame) if ok else None


class Reader:
    """Reads the resource panel from frames with the aoe2-squire binary."""

    def __init__(self, binary, tmp):
        if not Path(binary).exists():
            sys.exit(f"{binary} not found; build it with `cargo build --release` or pass --binary")
        self.binary = str(binary)
        self.tmp = Path(tmp)
        self.count = 0

    def save(self, frame):
        """Saves the frame with everything but the resource panel blacked out (BMP: much
        faster to write than PNG, and the files only live for one batch)."""
        h, w = frame.shape[:2]
        masked = frame * 0
        for (x0, x1), (y0, y1) in KEEP_REGIONS:
            ys, xs = slice(int(y0 * h), int(y1 * h)), slice(int(x0 * w), int(x1 * w))
            masked[ys, xs] = frame[ys, xs]
        path = self.tmp / f"{self.count:06d}.bmp"
        self.count += 1
        cv2.imwrite(str(path), masked)
        return path

    def read(self, frames):
        """Readings ({csv column: value}, or None without a panel) for an iterable of
        frames, read in batches as they come."""
        out, batch = [], []
        for frame in frames:
            batch.append(self.save(frame))
            if len(batch) == BATCH:
                out.extend(self._run(batch))
                batch = []
        if batch:
            out.extend(self._run(batch))
        return out

    def _run(self, paths):
        res = subprocess.run(
            [self.binary, *map(str, paths)], capture_output=True, text=True, check=True
        )
        blocks, current = {}, None
        for line in res.stdout.splitlines():
            if len(paths) > 1 and line.startswith("File: "):
                current = blocks.setdefault(line[6:].strip(), {})
            elif len(paths) == 1 and line.startswith("Image: "):
                current = blocks.setdefault(str(paths[0]), {})
            elif line.startswith("  ") and current is not None:
                field, _, value = line.strip().partition(": ")
                current[field] = value
        readings = []
        for p in paths:
            p.unlink()
            fields = blocks.get(str(p), {})
            reading = {col: fields.get(field, "") for col, field in FIELDS.items()}
            readings.append(reading if any(reading[c] for c in KEY) else None)
        return readings


def key_of(reading):
    if reading is None:
        return None
    key = tuple(reading[c] for c in KEY)
    return key if all(key) else None


def coarse_offset(samples, times, keys):
    """The offset most samples agree on, from the CSV rows showing each sample's key."""
    rows_by_key = {}
    for t, k in zip(times, keys):
        rows_by_key.setdefault(k, []).append(t)
    votes = Counter()
    for v, reading in samples:
        k = key_of(reading)
        bins = {(t - v) // VOTE_BIN_MS for t in rows_by_key.get(k, [])}
        votes.update(bins)
    if not votes:
        return None
    best, _ = votes.most_common(1)[0]
    # Center of the winning bin and its neighbours, weighted by votes.
    near = {b: votes[b] for b in (best - 1, best, best + 1)}
    return sum((b + 0.5) * VOTE_BIN_MS * n for b, n in near.items()) / sum(near.values())


def csv_key_at(times, keys, t):
    """Key of the CSV row in effect at time t (the last row at or before t)."""
    i = bisect_right(times, t) - 1
    return keys[i] if 0 <= i < len(keys) else None


def agreement(samples, times, keys, offset):
    """(matching, comparable): samples whose key equals the CSV row in effect at
    video time + offset or one row either side (the panel can change between rows)."""
    matching = comparable = 0
    for v, reading in samples:
        k = key_of(reading)
        t = v + offset
        if k is None or not times[0] <= t <= times[-1]:
            continue
        comparable += 1
        i = bisect_right(times, t) - 1
        if k in keys[max(0, i - 1) : i + 2]:
            matching += 1
    return matching, comparable


def find_changes(samples, times, keys, offset, window_ms=2500):
    """Pairs of consecutive samples showing two different keys, both found in the CSV near
    the coarse offset: (video time before, video time after, key before, key after)."""
    changes = []
    for (va, ra), (vb, rb) in zip(samples, samples[1:]):
        ka, kb = key_of(ra), key_of(rb)
        if ka is None or kb is None or ka == kb:
            continue
        lo = bisect_right(times, va + offset - window_ms)
        hi = bisect_right(times, vb + offset + window_ms)
        if ka in keys[lo:hi] and kb in keys[lo:hi]:
            changes.append((va, vb, ka, kb))
    return changes


def narrow_changes(video, reader, changes):
    """Narrows each change (v_old, v_new, k_old, k_new, ...) to two adjacent frames by
    bisection, one binary run per round. Drops changes where a frame in between has no
    readable panel."""
    open_changes = [list(c) for c in changes]
    done = []
    while open_changes:
        mids, still = [], []
        for c in open_changes:
            lo, hi = c[0], c[1]
            if hi - lo <= video.frame_ms * 1.5:
                done.append(tuple(c))
                continue
            got = video.frame_at((lo + hi) / 2)
            if got is None or not lo < got[0] < hi:
                done.append(tuple(c))  # cannot split further
                continue
            mids.append(got)
            still.append(c)
        if not still:
            break
        readings = reader.read([f for _, f in mids])
        open_changes = []
        for c, (t, _), reading in zip(still, mids, readings):
            k = key_of(reading)
            if k is None:
                continue
            if k == c[2]:
                c[0] = t
            else:
                # The new value, or a value in between (the panel changed more than once
                # between the two frames): narrow down the first change.
                c[1], c[3] = t, k
            open_changes.append(c)
    return done


def change_bounds(change, times, keys, offset, window_ms=2500):
    """Offset bounds from one narrowed change: the CSV's last row with the old key and first
    row with the new key around the change. None if the CSV does not show it cleanly."""
    v_old, v_new, k_old, k_new = change[:4]
    lo = bisect_right(times, v_old + offset - window_ms)
    hi = bisect_right(times, v_new + offset + window_ms)
    for i in range(lo, min(hi, len(times) - 1)):
        if keys[i] == k_old and keys[i + 1] == k_new:
            # The screen changed after times[i] and by times[i + 1] (real time), and after
            # v_old and by v_new (video time).
            return times[i] - v_new, times[i + 1] - v_old
    return None


def best_interval(bounds):
    """The offset range covered by the most bounds: (low, high, count)."""
    events = sorted([(lo, 1) for lo, _ in bounds] + [(hi, -1) for _, hi in bounds])
    best = (None, None, 0)
    depth = 0
    for i, (x, d) in enumerate(events):
        depth += d
        if d == 1 and depth > best[2]:
            nxt = events[i + 1][0] if i + 1 < len(events) else x
            best = (x, nxt, depth)
    return best


def main():
    parser = argparse.ArgumentParser(
        description=__doc__.split("\n\n")[0],
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="\n\n".join(__doc__.split("\n\n")[1:]),
    )
    parser.add_argument("video", type=Path)
    parser.add_argument("csv", type=Path)
    parser.add_argument("--at", action="append", default=[], help="time to convert (repeatable)")
    parser.add_argument("--offset-ms", type=float, help="skip measuring, use this offset")
    parser.add_argument("--step", type=float, default=2.0, help="coarse sampling step, seconds")
    parser.add_argument("--segments", type=int, default=4, help="parts checked for drift")
    parser.add_argument("--changes", type=int, default=8, help="changes narrowed per segment")
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--json", type=Path, help="also write the result to this file")
    args = parser.parse_args()

    times, keys = load_csv(args.csv)
    result = {"video": str(args.video), "csv": str(args.csv)}

    sidecar = args.video.with_suffix(".video.json")
    started = None
    if sidecar.exists():
        started = json.loads(sidecar.read_text()).get("ffmpeg_started_unix_ms")

    if args.offset_ms is not None:
        offset = args.offset_ms
    else:
        video = Video(args.video)
        print(
            f"Video {fmt_time(video.duration_ms)}, CSV {fmt_time(times[-1] - times[0])} "
            f"({len(times)} rows). Sampling every {args.step:g} s ..."
        )
        with tempfile.TemporaryDirectory(prefix="squire_align_") as tmp:
            reader = Reader(args.binary, tmp)
            sample_times = []

            def sampled():
                for t, frame in video.sample(args.step * 1000):
                    sample_times.append(t)
                    yield frame

            readings = reader.read(sampled())
            samples = list(zip(sample_times, readings))
            panel = sum(r is not None for _, r in samples)
            print(f"  {len(samples)} frames sampled, panel read on {panel}")

            coarse = coarse_offset(samples, times, keys)
            if coarse is None:
                sys.exit("No video frame matches a CSV row: is this the same game?")
            matching, comparable = agreement(samples, times, keys, coarse)
            print(
                f"Coarse offset {coarse / 1000:.1f} s: {matching} of {comparable} frames "
                f"inside the CSV agree with it"
            )

            changes = find_changes(samples, times, keys, coarse)
            # Split the part of the video that the CSV covers.
            start = max(0.0, times[0] - coarse)
            seg_len = (min(video.duration_ms, times[-1] - coarse) - start) / args.segments
            picked = []
            for s in range(args.segments):
                in_seg = [
                    c for c in changes if s * seg_len <= c[0] - start < (s + 1) * seg_len
                ]
                stride = max(1, len(in_seg) // args.changes)
                picked.extend((*c, s) for c in in_seg[::stride][: args.changes])
            narrowed = narrow_changes(video, reader, picked)

        by_segment = {}
        for change in narrowed:
            b = change_bounds(change, times, keys, coarse)
            if b is not None:
                by_segment.setdefault(change[4], []).append((change[0], b))

        print("\nOffset (CSV timestamp_ms - video ms) by part of the video:")
        segments, all_bounds = [], []
        for seg in sorted(by_segment):
            items = by_segment[seg]
            bounds = [b for _, b in items]
            all_bounds.extend(bounds)
            lo, hi, n = best_interval(bounds)
            mid = (lo + hi) / 2
            span = (min(v for v, _ in items), max(v for v, _ in items))
            segments.append({"video_ms": span, "offset_ms": mid, "window_ms": hi - lo,
                             "changes": n, "of": len(bounds)})
            print(
                f"  video {fmt_time(span[0])}-{fmt_time(span[1])}: {mid:.0f} ms "
                f"(window {hi - lo:.0f} ms, {n} of {len(bounds)} changes agree)"
            )
        if not all_bounds:
            print("  no usable changes; using the coarse offset")
            offset = coarse
        else:
            lo, hi, n = best_interval(all_bounds)
            offset = (lo + hi) / 2
            print(
                f"All parts: {offset:.0f} ms, window {hi - lo:.0f} ms "
                f"({n} of {len(all_bounds)} changes agree)"
            )
            mids = [s["offset_ms"] for s in segments]
            drift = max(mids) - min(mids)
            overlap = n == len(all_bounds)
            print(
                f"Drift: parts differ by {drift:.0f} ms"
                + (" (all changes fit one offset: no drift)" if overlap else "")
            )
            result.update(segments=segments, window_ms=hi - lo, drift_ms=drift)
        result["agreement"] = [matching, comparable]

    result["offset_ms"] = offset
    print(f"\nOffset: {offset:.0f} ms  (video time = CSV timestamp_ms - offset)")
    print(f"First CSV row (match start) is at video {fmt_time(times[0] - offset)}")
    if started is not None:
        print(
            f"Sidecar: ffmpeg was started {offset - started:.0f} ms before the first video "
            f"frame"
        )
        result["ffmpeg_start_to_first_frame_ms"] = offset - started

    conversions = []
    for at in args.at:
        t = parse_at(at, times[0])
        conversions.append({"at": at, "csv_ms": t, "video_ms": t - offset})
        print(f"  {at} -> CSV {t} (match {fmt_time(t - times[0])}) -> video {fmt_time(t - offset)}")
    result["conversions"] = conversions
    if args.json:
        args.json.write_text(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
