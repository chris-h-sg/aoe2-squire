import unittest
import subprocess
import csv
import os
import sys
import cv2
from pathlib import Path

class TestVideoPOC(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Setup paths relative to script location
        cls.research_dir = Path(__file__).parent.absolute()
        cls.root_dir = cls.research_dir.parent
        cls.poc_script = cls.research_dir / "poc_video.py"
        cls.output_csv = cls.research_dir / "output" / "test_video_results.csv"

    def get_test_cases(self):
        """Returns a list of (video_filename, interval) to test."""
        return [
            ("extract-housed.mp4", 1.0),
            ("extract-housed.mp4", 0.5),
            ("extract.mp4", 1.0),
            ("extract2.mp4", 1.0),
        ]

    def test_video_processing_accuracy(self):
        """Runs poc_video.py and compares the output CSV to expected values for multiple cases."""
        test_cases = self.get_test_cases()
        
        for video_filename, interval in test_cases:
            with self.subTest(video=video_filename, interval=interval):
                video_path = self.root_dir / "test_bench" / video_filename
                
                # Construct expected filename: <video_name>_expected_<interval>s.csv
                # Note: The user specified <video name>_expected_<interval>.csv
                # Looking at filenames: extract2_expected_1s.csv
                video_stem = Path(video_filename).stem
                expected_filename = f"{video_stem}_expected_{interval:g}s.csv"
                expected_csv = self.root_dir / "test_bench" / expected_filename
                
                if not video_path.exists():
                    print(f"Skipping {video_filename}: Video not found")
                    continue
                if not expected_csv.exists():
                    print(f"Skipping {video_filename}: Expected file {expected_filename} not found")
                    continue

                # 1. Run the POC video script
                cmd = [
                    sys.executable, str(self.poc_script),
                    "--video", str(video_path),
                    "--output", str(self.output_csv),
                    "--interval", str(interval)
                ]
                
                print(f"\nTesting {video_filename} at {interval}s interval...")
                result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(self.research_dir))
                
                if result.returncode != 0:
                    print(f"STDOUT: {result.stdout}")
                    print(f"STDERR: {result.stderr}")
                    
                self.assertEqual(result.returncode, 0, f"poc_video.py failed for {video_filename}")

                # 2. Check if output file exists
                self.assertTrue(self.output_csv.exists(), f"Output CSV was not created for {video_filename}")

                # 3. Compare the results
                with open(expected_csv, 'r', newline='') as f_exp, \
                     open(self.output_csv, 'r', newline='') as f_act:
                    
                    reader_exp = list(csv.DictReader(f_exp))
                    reader_act = list(csv.DictReader(f_act))

                    # Filter out empty rows
                    reader_exp = [r for r in reader_exp if any(r.values())]
                    reader_act = [r for r in reader_act if any(r.values())]

                    self.assertEqual(len(reader_exp), len(reader_act), 
                                     f"Row count mismatch for {video_filename}: expected {len(reader_exp)}, got {len(reader_act)}")

                    mismatches = []
                    saved_frames = set()
                    
                    for i, (row_exp, row_act) in enumerate(zip(reader_exp, reader_act)):
                        row_mismatches = []
                        for key in row_exp.keys():
                            val_exp = row_exp[key].strip()
                            val_act = row_act.get(key, "").strip()
                            
                            if val_act != val_exp:
                                row_mismatches.append(f"Col '{key}': Exp '{val_exp}', Got '{val_act}'")
                        
                        if row_mismatches:
                            timestamp_str = row_exp['timestamp_sec']
                            try:
                                timestamp = float(timestamp_str)
                            except ValueError:
                                timestamp = 0.0
                            
                            frame_idx = int(row_act.get('frame_idx', 0))
                            
                            mismatches.append(f"Row {i} ({timestamp_str}s): {', '.join(row_mismatches)}")
                            
                            if timestamp_str not in saved_frames:
                                frame_path = self.save_error_frame(video_path, frame_idx, timestamp, video_stem)
                                if frame_path:
                                    mismatches.append(f"  [Frame saved: {frame_path.name}]")
                                saved_frames.add(timestamp_str)

                    if mismatches:
                        error_msg = "\n".join(mismatches)
                        self.fail(f"Found mismatches in {video_filename}:\n{error_msg}")

                # Clean up between subtests
                if self.output_csv.exists():
                    os.remove(self.output_csv)

    def save_error_frame(self, video_path, frame_idx, timestamp, video_stem):
        """Saves the frame at frame_idx to research/output/error_frames/."""
        error_frames_dir = self.research_dir / "output" / "error_frames"
        error_frames_dir.mkdir(parents=True, exist_ok=True)
        
        cap = cv2.VideoCapture(str(video_path))
        if not cap.isOpened():
            return None
        
        cap.set(cv2.CAP_PROP_POS_FRAMES, frame_idx)
        ret, frame = cap.read()
        if ret:
            filename = f"{video_stem}_err_{timestamp:.2f}s_f{frame_idx}.png"
            filepath = error_frames_dir / filename
            cv2.imwrite(str(filepath), frame)
            cap.release()
            # Return relative path for cleaner output
            return filepath
        
        cap.release()
        return None

    def tearDown(self):
        # Final cleanup
        if self.output_csv.exists():
            try:
                os.remove(self.output_csv)
            except OSError:
                pass

if __name__ == "__main__":
    unittest.main()
