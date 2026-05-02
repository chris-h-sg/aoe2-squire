import cv2
import csv
import os
import argparse
import json
import time
from pathlib import Path

# Import functions from the existing vision POC
import poc_vision

def process_video(video_path, output_csv, interval_sec, ui_map, templates, save_frames=False, frames_dir=None):
    cap = cv2.VideoCapture(video_path)
    if not cap.isOpened():
        print(f"Error: Could not open video {video_path}")
        return

    fps = cap.get(cv2.CAP_PROP_FPS)
    total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    
    # Early escape if FPS is invalid
    if fps <= 0:
        print(f"Error: Invalid FPS {fps} detected in video.")
        cap.release()
        return

    duration = total_frames / fps
    
    print(f"Processing Video: {video_path}")
    print(f"Properties: {fps:.2f} FPS, {total_frames} total frames, {duration:.2f}s duration")
    print(f"Interval: {interval_sec}s")
    print(f"Output: {output_csv}")
    if save_frames:
        print(f"Saving frames to: {frames_dir}")
        os.makedirs(frames_dir, exist_ok=True)

    # Prepare CSV headers based on ui_map elements
    # We'll use the names exactly as they are in the JSON
    element_names = list(ui_map.get('elements', {}).keys())
    headers = ['timestamp_sec', 'frame_idx'] + element_names + ['pop_color', 'housing']

    results = []
    
    # Open CSV early to write headers and rows as we go (safer for long videos)
    with open(output_csv, 'w', newline='') as csvfile:
        writer = csv.DictWriter(csvfile, fieldnames=headers)
        writer.writeheader()

        next_process_time = 0.0
        frame_idx = 0

        while True:
            ret, frame = cap.read()
            if not ret:
                break

            timestamp = frame_idx / fps
            
            # Use a small epsilon to avoid floating point issues
            if timestamp >= next_process_time - 0.001:
                print(f"  [{timestamp:6.2f}s] Processing frame {frame_idx}...", end='', flush=True)
                t_frame_start = time.time()
                
                # Save frame if requested
                if save_frames:
                    frame_filename = os.path.join(frames_dir, f"frame_{timestamp:07.2f}s.png")
                    cv2.imwrite(frame_filename, frame)

                # Process the frame using poc_vision
                results = poc_vision.process_frame(frame, ui_map, templates, verbose=False)

                if results is None:
                    t_frame_end = time.time()
                    print(f" [no anchor] ({(t_frame_end - t_frame_start)*1000:.0f}ms)")
                else:
                    # Convert results to CSV row format
                    row_data = {
                        'timestamp_sec': round(timestamp, 2),
                        'frame_idx': frame_idx
                    }

                    # Flatten the nested results dict to match CSV headers
                    for name in element_names:
                        parts = name.split('_')
                        category = parts[0]
                        sub_key = parts[1] if len(parts) > 1 else "value"
                        row_data[name] = results.get(category, {}).get(sub_key, "")

                    pop_color = results.get('population', {}).get('color', 'white')
                    row_data['pop_color'] = pop_color
                    
                    if pop_color == 'white':
                        row_data['housing'] = 'normal'
                    elif pop_color == 'yellow':
                        row_data['housing'] = 'queued'
                    elif pop_color == 'overlay':
                        row_data['housing'] = 'housed'
                    else:
                        row_data['housing'] = 'normal'

                    writer.writerow(row_data)
                    csvfile.flush()

                    t_frame_end = time.time()
                    print(f" Done ({(t_frame_end - t_frame_start)*1000:.0f}ms)")
                
                # Advance to next scheduled timestamp
                next_process_time += interval_sec

            frame_idx += 1

    cap.release()
    print(f"\nProcessing complete! Results saved to: {output_csv}")

def main():
    parser = argparse.ArgumentParser(description="POC Video Analyzer: Extract data points from game footage.")
    parser.add_argument("--video", type=str, default="../test_bench/extract.mp4", help="Path to input video file")
    parser.add_argument("--interval", type=float, default=1.0, help="Sampling interval in seconds (default: 1.0)")
    parser.add_argument("--output", type=str, default="extract_results.csv", help="Output CSV filename")
    parser.add_argument("--save-frames", action="store_true", help="Save the sampled frames to output/frames/")
    args = parser.parse_args()

    # Setup paths relative to script location
    script_dir = Path(__file__).parent.absolute()
    
    # 1. Load UI Map
    map_path = script_dir.parent / "ui_map.json"
    if not map_path.exists():
        print(f"Error: UI map not found at {map_path}")
        return
    with open(map_path, 'r') as f:
        ui_map = json.load(f)

    # 2. Load Templates (using enormous_numbers as the primary source)
    templates_dir = script_dir / "templates" / "enormous_numbers"
    if not templates_dir.exists():
        print(f"Error: Templates directory not found at {templates_dir}")
        return
        
    templates = poc_vision.load_templates(str(templates_dir))
    if not templates:
        print(f"Error: No templates loaded from {templates_dir}")
        return
        
    print(f"Successfully loaded {len(templates)} digit templates.")

    # 3. Resolve Video Path
    video_path = Path(args.video)
    if not video_path.is_absolute():
        video_path = (script_dir / args.video).resolve()
        
    if not video_path.exists():
        print(f"Error: Video file not found at {video_path}")
        return

    # 4. Resolve Output Path
    output_path = Path(args.output)
    if not output_path.is_absolute():
        output_path = script_dir / "output" / args.output
    
    # Ensure output directory exists
    output_path.parent.mkdir(parents=True, exist_ok=True)

    # 5. Setup frames directory
    frames_dir = script_dir / "output" / "frames"

    # Run the core logic
    process_video(str(video_path), str(output_path), args.interval, ui_map, templates, args.save_frames, str(frames_dir))

if __name__ == "__main__":
    main()
