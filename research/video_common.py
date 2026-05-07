import json
import os
from pathlib import Path
from typing import Optional, Dict, Any
import poc_vision

def parse_time(time_str: Optional[str]) -> Optional[float]:
    """Parses time string in format [HH:]MM:SS or just SS into seconds."""
    if not time_str:
        return None
    try:
        if ':' in time_str:
            parts = time_str.split(':')
            if len(parts) == 2:
                return int(parts[0]) * 60 + float(parts[1])
            elif len(parts) == 3:
                return int(parts[0]) * 3600 + int(parts[1]) * 60 + float(parts[2])
        return float(time_str)
    except ValueError:
        return None

def load_ui_map(script_dir: Path) -> Dict[str, Any]:
    map_path = script_dir.parent / "ui_map.json"
    if not map_path.exists():
        raise FileNotFoundError(f"UI map not found at {map_path}")
    with open(map_path, 'r') as f:
        return json.load(f)

def load_templates(script_dir: Path) -> Dict[str, Any]:
    templates_dir = script_dir / "templates" / "enormous_numbers"
    if not templates_dir.exists():
        raise FileNotFoundError(f"Templates directory not found at {templates_dir}")
    templates = poc_vision.load_templates(str(templates_dir))
    if not templates:
        raise ValueError(f"No templates loaded from {templates_dir}")
    return templates
