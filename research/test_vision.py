import unittest
import os
import json
import cv2
import sys
import numpy as np

# Append the directory containing poc_vision to sys.path
sys.path.append(os.path.join(os.getcwd(), 'research'))

import poc_vision

class TestVisionSystem(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Paths
        cls.test_bench_dir = "test_bench"
        cls.ui_map_path = "ui_map.json"
        cls.templates_dir = os.path.join("research", "templates", "enormous_numbers")
        cls.expected_values_path = os.path.join("test_bench", "expected_values.json")
        
        # Load Resources
        with open(cls.ui_map_path, 'r') as f:
            cls.ui_map = json.load(f)
            
        with open(cls.expected_values_path, 'r') as f:
            cls.all_expected = json.load(f)
            
        cls.templates = poc_vision.load_templates(cls.templates_dir)

    def test_detect_ui_scale_approximation(self):
        """Test if the UI scale is detected within a reasonable margin for known images."""
        expected_scales = {
            "aoe2_16x9_max.png": 1.00,
        }
        
        for filename, expected_scale in expected_scales.items():
            image_path = os.path.join(self.test_bench_dir, filename)
            if not os.path.exists(image_path):
                continue
                
            img = cv2.imread(image_path)
            scale = poc_vision.detect_ui_scale(img)
            with self.subTest(image=filename):
                self.assertAlmostEqual(scale, expected_scale, delta=0.05, 
                                     msg=f"UI Scale deviation for {filename}")

    def get_allowed_images(self):
        return {
            "aoe2_16x9.png", 
            "aoe2_16x9_min.png", 
            "aoe2_16x9_max.png",
            "aoe2_4k.png",
            "aoe2_16x10.png",
            "aoe2_21x9.png", 
            "aoe2_32x9.png",
            "housed_no_overlay.png",
            "housed_overlay.png",
            "aoe2_1366x768_Anne_HK.png",
            "aoe2_16x9_Anne_HK.png",
            "aoe2_16x9_Anne_HK-no-idle.png",
            "aoe2_16x9_Anne_HK-103-idle.png",
            "7_vil_Anne_HK.png",
            "7_vil_Anne_HK-2.png"
        }

    def test_extract_digits_count_parameterized(self):
        """Test if the extractor finds the correct number of digits in a known box (All allowed)."""
        allowed = self.get_allowed_images()
        
        for filename in allowed:
            image_path = os.path.join(self.test_bench_dir, filename)
            if not os.path.exists(image_path):
                continue

            with self.subTest(image=filename):
                img = cv2.imread(image_path)
                scale = poc_vision.detect_ui_scale(img)
                
                # Get expected value for wood_total
                config = self.all_expected['images'].get(filename, {})
                set_name = config.get('use_value_set')
                expected_val = self.all_expected['value_sets'][set_name]['wood']['total']
                expected_count = len(expected_val)

                # Coords from ui_map.json for 'wood_total'
                coords = self.ui_map['elements']['wood_total']
                x = int(coords['x_px'] * scale)
                y = int(coords['y_px'] * scale)
                w = int(coords['w_px'] * scale)
                h = int(coords['h_px'] * scale)
                
                if x+w > img.shape[1] or y+h > img.shape[0]:
                    self.fail(f"Coordinates out of bounds for {filename}")

                box_img = img[y:y+h, x:x+w].copy()
                digits = poc_vision.extract_digits(box_img, scale)
                
                self.assertEqual(len(digits), expected_count, 
                               f"Digit count mismatch for {filename} (Expected {expected_count})")

    def test_digit_matching_parameterized(self):
        """Test if specific digits are matched correctly (All allowed)."""
        allowed = self.get_allowed_images()
        
        for filename in allowed:
            image_path = os.path.join(self.test_bench_dir, filename)
            if not os.path.exists(image_path):
                continue
                
            with self.subTest(image=filename):
                img = cv2.imread(image_path)
                scale = poc_vision.detect_ui_scale(img)
                
                # Get expected value for wood_total
                config = self.all_expected['images'].get(filename, {})
                set_name = config.get('use_value_set')
                expected_str = self.all_expected['value_sets'][set_name]['wood']['total']
                
                coords = self.ui_map['elements']['wood_total']
                x = int(coords['x_px'] * scale)
                y = int(coords['y_px'] * scale)
                w = int(coords['w_px'] * scale)
                h = int(coords['h_px'] * scale)
                
                box_img = img[y:y+h, x:x+w].copy()
                digits = poc_vision.extract_digits(box_img, scale)
                
                detected_str = ""
                for i, digit_img in enumerate(digits):
                    char, conf, _ = poc_vision.match_digit_to_template(digit_img, self.templates)
                    detected_str += char
                    
                self.assertEqual(detected_str, expected_str, 
                               f"Mismatch in digit matching for {filename}")

    def test_all_images_end_to_end(self):
        """
        Iterates through allowed images in expected_values.json and verifies the vision pipeline.
        """
        images_config = self.all_expected.get('images', {})
        value_sets = self.all_expected.get('value_sets', {})
        
        # User requested specific whitelist for now
        allowed_images = self.get_allowed_images()
        
        for filename, config in images_config.items():
            if filename not in allowed_images:
                continue

            image_path = os.path.join(self.test_bench_dir, filename)
            
            # Skip if file doesn't exist locally (don't fail the test suite for missing local files)
            if not os.path.exists(image_path):
                print(f"Skipping {filename} (File not found)")
                continue

            with self.subTest(image=filename):
                print(f"Testing {filename}...")
                
                # Run Pipeline
                results = poc_vision.process_image(image_path, self.ui_map, self.templates)
                
                # Get Expected Data
                set_name = config.get('use_value_set')
                expected_data = value_sets.get(set_name)
                
                self.assertIsNotNone(expected_data, f"Value set '{set_name}' not found for {filename}")
                
                # Compare extracted results against expected values key-by-key
                for resource, details in expected_data.items():
                    if resource not in results:
                        self.fail(f"Resource '{resource}' not detected in {filename}")
                        
                    for key, val in details.items():
                        if key not in results[resource]:
                            self.fail(f"Key '{resource}.{key}' not detected in {filename}")
                            
                        self.assertEqual(results[resource][key], val, 
                                       f"Mismatch in {filename} for {resource}.{key}")

if __name__ == '__main__':
    unittest.main(buffer=False) # Disable buffer to see progress prints
