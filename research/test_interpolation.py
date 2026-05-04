import csv
import os
from pathlib import Path
from interpolation_engine import InterpolationEngine, CapturedFrame

def run_test(name, scenario_csv):
    print(f"Running test: {name}...")
    engine = InterpolationEngine(timeout_sec=1.0)
    
    expected_housing = []
    actual_rows = []
    
    with open(scenario_csv, 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            # Save expected result
            expected_housing.append(row['housing'])
            
            # Reset housing to 'raw' state based on color (as poc_video.py does)
            pop_color = row['pop_color']
            if pop_color == 'white':
                row['housing'] = 'normal'
            elif pop_color == 'yellow':
                row['housing'] = 'queued'
            elif pop_color == 'overlay':
                row['housing'] = 'housed'
            else:
                row['housing'] = 'normal'

            frame = CapturedFrame(
                timestamp=float(row['timestamp_sec']),
                frame_idx=int(row['frame_idx']),
                pop_color=pop_color,
                pop_total=int(row['population_total']),
                pop_housing=int(row['population_housing']),
                row_data=row.copy()
            )
            actual_rows.extend(engine.process_frame(frame))
    
    actual_rows.extend(engine.flush())
    
    if len(actual_rows) != len(expected_housing):
        print(f"  FAILED: Row count mismatch. Actual: {len(actual_rows)}, Expected: {len(expected_housing)}")
        return False
        
    failed = False
    for i, (actual, expected) in enumerate(zip(actual_rows, expected_housing)):
        if actual['housing'] != expected:
            print(f"  FAILED at row {i} (t={actual['timestamp_sec']}s):")
            print(f"    Expected housing: {expected}")
            print(f"    Actual housing:   {actual['housing']}")
            failed = True
            
    if not failed:
        print("  PASSED")
        return True
    return False

def main():
    # Test data is now located in test_bench/interpolation/
    test_data_dir = Path(__file__).parent.parent / "test_bench" / "interpolation"
    scenarios = [
        "flicker",
        "house_completion",
        "unit_death",
        "timeout"
    ]
    
    all_passed = True
    for scenario in scenarios:
        scenario_file = test_data_dir / f"{scenario}.csv"
        
        if not scenario_file.exists():
            print(f"Skipping {scenario}: missing file")
            continue
            
        if not run_test(scenario, scenario_file):
            all_passed = False
            
    if all_passed:
        print("\nALL TESTS PASSED!")
    else:
        print("\nSOME TESTS FAILED.")

if __name__ == "__main__":
    main()
