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
        cls.test_video = cls.root_dir / "test_bench" / "extract.mp4"
        cls.output_csv = cls.research_dir / "output" / "test_video_results.csv"
        cls.expected_csv = cls.root_dir / "test_bench" / "expected_extract_results.csv"

        # Ensure we are running from a predictable location
        # The poc_video.py script expects things like templates in research/templates
        # and ui_map.json in the parent directory.
        
    def test_video_processing_accuracy(self):
        """Runs poc_video.py and compares the output CSV to expected values."""
        
        # 1. Run the POC video script
        # We run it from the research directory to ensure relative paths inside it work as expected
        cmd = [
            sys.executable, str(self.poc_script),
            "--video", str(self.test_video),
            "--output", str(self.output_csv),
            "--interval", "1.0"
        ]
        
        print(f"Running command: {' '.join(cmd)}")
        result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(self.research_dir))
        
        if result.returncode != 0:
            print(f"STDOUT: {result.stdout}")
            print(f"STDERR: {result.stderr}")
            
        self.assertEqual(result.returncode, 0, f"poc_video.py failed with return code {result.returncode}")

        # 2. Check if output file exists
        self.assertTrue(self.output_csv.exists(), f"Output CSV was not created at {self.output_csv}")

        # 3. Compare the results
        with open(self.expected_csv, 'r', newline='') as f_exp, \
             open(self.output_csv, 'r', newline='') as f_act:
            
            reader_exp = list(csv.DictReader(f_exp))
            reader_act = list(csv.DictReader(f_act))

            # Filter out empty rows if any
            reader_exp = [r for r in reader_exp if any(r.values())]
            reader_act = [r for r in reader_act if any(r.values())]

            self.assertEqual(len(reader_exp), len(reader_act), 
                             f"Row count mismatch: expected {len(reader_exp)}, got {len(reader_act)}")

            mismatches = []
            for i, (row_exp, row_act) in enumerate(zip(reader_exp, reader_act)):
                # Check all columns present in expected
                for key in row_exp.keys():
                    val_exp = row_exp[key].strip()
                    val_act = row_act.get(key, "").strip()
                    
                    if val_act != val_exp:
                        mismatches.append(f"Row {i} ({row_exp['timestamp_sec']}s), Column '{key}': Expected '{val_exp}', Got '{val_act}'")

            if mismatches:
                error_msg = "\n".join(mismatches)
                self.fail(f"Found {len(mismatches)} mismatches in video results:\n{error_msg}")

    def tearDown(self):
        # Clean up the output CSV after test
        if self.output_csv.exists():
            try:
                os.remove(self.output_csv)
            except OSError:
                pass

if __name__ == "__main__":
    unittest.main()
