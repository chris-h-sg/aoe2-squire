import cv2
import os
import argparse
import time
from pathlib import Path
from typing import Optional

import video_common

# We'll import these dynamically based on --engine
import poc_vision
import vision_rust_aligned

def process_video(video_path, interval_sec, ui_map, templates, frames_dir, debug_target=None, debug_key="population_vils", start_time=None, end_time=None, engine=vision_rust_aligned):
    cap = cv2.VideoCapture(video_path)
    if not cap.isOpened():
        print(f"Error: Could not open video {video_path}")
        return

    fps = cap.get(cv2.CAP_PROP_FPS)
    total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    
    if fps <= 0:
        print(f"Error: Invalid FPS {fps} detected in video.")
        cap.release()
        return

    duration = total_frames / fps
    
    print(f"Extracting Outliers from: {video_path}")
    print(f"Properties: {fps:.2f} FPS, {total_frames} total frames, {duration:.2f}s duration")
    print(f"Interval: {interval_sec}s")
    if start_time:
        print(f"Start Time: {start_time}s")
    if end_time:
        print(f"End Time: {end_time}s")
    print(f"Monitoring: {debug_key} (expected: {debug_target})")
    print(f"Output Directory: {frames_dir}")
    os.makedirs(frames_dir, exist_ok=True)

    next_process_time = 0.0
    frame_idx = 0

    if start_time:
        cap.set(cv2.CAP_PROP_POS_MSEC, start_time * 1000)
        frame_idx = int(start_time * fps)
        next_process_time = start_time

    while True:
        ret, frame = cap.read()
        if not ret:
            break

        timestamp = frame_idx / fps
        
        if end_time and timestamp > end_time:
            print(f"\nReached end time {end_time}s. Stopping.")
            break

        if timestamp >= next_process_time - 0.001:
            print(f"  [{timestamp:6.2f}s] Frame {frame_idx}...", end='', flush=True)
            t_frame_start = time.time()
            
            # Process the frame using selected engine
            results = engine.process_frame(frame, ui_map, templates, verbose=False)

            if results is None:
                # SAVE IF ANCHOR LOST
                frame_filename = os.path.join(frames_dir, f"lost_anchor_{timestamp:07.2f}s.png")
                cv2.imwrite(frame_filename, frame)
                print(f" [ANCHOR LOST - SAVED]", end='')
            else:
                # Flat check for outliers
                parts = debug_key.split('_')
                category = parts[0]
                sub_key = parts[1] if len(parts) > 1 else "value"
                
                # Special case for population_total splitting
                if debug_key == "population_total":
                    total = results.get('population', {}).get('total', "")
                    housing = results.get('population', {}).get('housing', "")
                    current_val = f"{total}/{housing}" if housing else total
                else:
                    current_val = results.get(category, {}).get(sub_key, "")

                if current_val != debug_target:
                    # Sanitize filename (replace slash with 'slash' and remove other bad chars)
                    safe_val = current_val.replace('/', 'slash').replace('?', 'unknown')
                    frame_filename = os.path.join(frames_dir, f"outlier_{timestamp:07.2f}s_val_{safe_val}.png")
                    cv2.imwrite(frame_filename, frame)
                    print(f" [SAVED: {current_val}]", end='')
                else:
                    print(f" [ok]", end='')

            t_frame_end = time.time()
            print(f" ({(t_frame_end - t_frame_start)*1000:.0f}ms)")
            
            next_process_time += interval_sec

        frame_idx += 1

    cap.release()
    print(f"\nExtraction complete!")

def main():
    parser = argparse.ArgumentParser(description="Extract outlier frames from video for OCR debugging.")
    parser.add_argument("--video", type=str, required=True, help="Path to input video file")
    parser.add_argument("--target", type=str, required=True, help="The expected OCR value (e.g. 52)")
    parser.add_argument("--key", type=str, default="population_vils", help="The UI element key to monitor (default: population_vils)")
    parser.add_argument("--interval", type=float, default=0.5, help="Sampling interval in seconds (default: 0.5)")
    parser.add_argument("--start", type=str, help="Start time (e.g. 18:55)")
    parser.add_argument("--end", type=str, help="End time (e.g. 19:25)")
    parser.add_argument("--outdir", type=str, default="output/outliers", help="Directory to save frames")
    parser.add_argument("--engine", type=str, choices=["poc", "rust"], default="rust", help="The vision engine to use (default: rust)")
    args = parser.parse_args()

    engine = vision_rust_aligned if args.engine == "rust" else poc_vision

    script_dir = Path(__file__).parent.absolute()
    
    try:
        ui_map = video_common.load_ui_map(script_dir)
        # Use the engine-specific loader to ensure canvas parity
        templates = engine.load_templates(str(script_dir / "templates" / "enormous_numbers"))
    except Exception as e:
        print(f"Error: {e}")
        return

    start_seconds = video_common.parse_time(args.start)
    end_seconds = video_common.parse_time(args.end)

    # Resolve Video Path
    video_path = Path(args.video)
    if not video_path.is_absolute():
        video_path = (script_dir / args.video).resolve()
    if not video_path.exists():
        print(f"Error: Video file not found at {video_path}")
        return

    # Resolve Output Path
    out_dir = Path(args.outdir)
    if not out_dir.is_absolute():
        out_dir = script_dir / args.outdir

    process_video(str(video_path), args.interval, ui_map, templates, str(out_dir), args.target, args.key, start_seconds, end_seconds, engine=engine)

if __name__ == "__main__":
    main()
