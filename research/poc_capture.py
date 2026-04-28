import cv2
import json
import time
import argparse
import numpy as np
from pathlib import Path

import mss
import poc_vision

# dxcam is faster (~DirectX desktop duplication) but fails on some exclusive-fullscreen
# configs and requires a separate install. mss is the reliable default.


def capture_loop(fps_target, ui_map, templates, verbose):
    interval = 1.0 / fps_target
    sct = mss.mss()
    monitor = sct.monitors[1]  # primary monitor; index 0 is the virtual full-desktop

    print(f"Capturing primary monitor: {monitor['width']}x{monitor['height']}")
    print(f"Target rate: {fps_target} fps  (interval: {interval*1000:.0f}ms)")
    print(f"Press Ctrl+C to stop.\n")

    sample_idx = 0
    timing_history = []

    try:
        while True:
            loop_start = time.perf_counter()

            # --- Capture ---
            t0 = time.perf_counter()
            raw = sct.grab(monitor)
            t_capture = (time.perf_counter() - t0) * 1000

            # mss returns BGRA; drop alpha channel for OpenCV
            frame = np.array(raw)[:, :, :3]

            # --- Process ---
            t1 = time.perf_counter()
            results = poc_vision.process_frame(frame, ui_map, templates, verbose=verbose)
            t_process = (time.perf_counter() - t1) * 1000

            t_total = (time.perf_counter() - loop_start) * 1000

            if results is None:
                print(f"[{sample_idx:4d}] [no anchor]  | capture={t_capture:.0f}ms  total={t_total:.0f}ms")
            else:
                timing_history.append(t_process)
                res_str = _format_results(results)
                print(
                    f"[{sample_idx:4d}] {res_str}"
                    f"  | capture={t_capture:.0f}ms  process={t_process:.0f}ms  total={t_total:.0f}ms"
                )

            sample_idx += 1

            # --- Sleep remainder of interval ---
            elapsed = time.perf_counter() - loop_start
            sleep_for = interval - elapsed
            if sleep_for > 0:
                time.sleep(sleep_for)

    except KeyboardInterrupt:
        pass

    if timing_history:
        avg = sum(timing_history) / len(timing_history)
        peak = max(timing_history)
        budget = interval * 1000
        print(
            f"\n--- Timing summary ({sample_idx} samples) ---\n"
            f"  Process avg : {avg:.1f}ms\n"
            f"  Process peak: {peak:.1f}ms\n"
            f"  Frame budget: {budget:.0f}ms  ({fps_target} fps)\n"
            f"  Budget used : {avg/budget*100:.1f}% avg  /  {peak/budget*100:.1f}% peak"
        )


def _format_results(results):
    """Compact one-liner of extracted values."""
    parts = []
    order = ["food", "wood", "gold", "stone", "population", "idle"]
    for key in order:
        if key in results:
            inner = results[key]
            for sub, val in inner.items():
                parts.append(f"{key[:3]}_{sub[:3]}={val or '?'}")
    if not parts:
        parts = ["(no data)"]
    return "  ".join(parts)


def main():
    parser = argparse.ArgumentParser(
        description="POC Live Capture: run vision pipeline against the primary monitor."
    )
    parser.add_argument(
        "--fps", type=float, default=2.0,
        help="Target sampling rate in frames/sec (default: 2.0)"
    )
    parser.add_argument(
        "--verbose", action="store_true",
        help="Show per-element debug output from process_frame()"
    )
    args = parser.parse_args()

    script_dir = Path(__file__).parent.absolute()

    map_path = script_dir.parent / "ui_map.json"
    if not map_path.exists():
        print(f"Error: UI map not found at {map_path}")
        return
    with open(map_path, "r") as f:
        ui_map = json.load(f)

    templates_dir = script_dir / "templates" / "enormous_numbers"
    if not templates_dir.exists():
        print(f"Error: Templates directory not found at {templates_dir}")
        return
    templates = poc_vision.load_templates(str(templates_dir))
    if not templates:
        print(f"Error: No templates loaded from {templates_dir}")
        return

    print(f"Loaded {len(templates)} digit templates.")
    capture_loop(args.fps, ui_map, templates, verbose=args.verbose)


if __name__ == "__main__":
    main()
