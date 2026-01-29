import unittest
import subprocess
import csv
import os
import sys
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
                    for i, (row_exp, row_act) in enumerate(zip(reader_exp, reader_act)):
                        for key in row_exp.keys():
                            val_exp = row_exp[key].strip()
                            val_act = row_act.get(key, "").strip()
                            
                            if val_act != val_exp:
                                mismatches.append(f"Row {i} ({row_exp['timestamp_sec']}s), Column '{key}': Expected '{val_exp}', Got '{val_act}'")

                    if mismatches:
                        error_msg = "\n".join(mismatches)
                        self.fail(f"Found {len(mismatches)} mismatches in {video_filename}:\n{error_msg}")

                # Clean up between subtests
                if self.output_csv.exists():
                    os.remove(self.output_csv)

    def tearDown(self):
        # Final cleanup
        if self.output_csv.exists():
            try:
                os.remove(self.output_csv)
            except OSError:
                pass

if __name__ == "__main__":
    unittest.main()
