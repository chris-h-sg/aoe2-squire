from dataclasses import dataclass
from typing import Dict, Any, List, Optional

@dataclass
class CapturedFrame:
    timestamp: float
    frame_idx: int
    pop_color: str
    pop_total: int
    pop_housing: int
    row_data: Dict[str, Any]

    @property
    def pop_diff(self) -> int:
        return self.pop_housing - self.pop_total

class InterpolationEngine:
    def __init__(self, timeout_sec: float = 1.0):
        self.timeout_sec = timeout_sec
        self.pending_buffer: List[CapturedFrame] = []
        self.last_housed_anchor: Optional[CapturedFrame] = None

    def process_frame(self, frame: CapturedFrame) -> List[Dict[str, Any]]:
        """
        Processes a single frame and returns a list of row_data to be written to output.
        Some frames might be buffered, so this might return an empty list or multiple rows.
        """
        output_rows = []

        if frame.pop_color == 'overlay':
            # Check if we have a previous anchor to bridge to
            if self.last_housed_anchor and (frame.timestamp - self.last_housed_anchor.timestamp <= self.timeout_sec):
                # Bridge found: evaluate intermediate frames
                max_diff_bound = max(self.last_housed_anchor.pop_diff, frame.pop_diff)
                for bf in self.pending_buffer:
                    if bf.pop_diff <= max_diff_bound:
                        bf.row_data['housing'] = 'housed'
            
            # Flush any processed buffer and the current anchor
            for bf in self.pending_buffer:
                output_rows.append(bf.row_data)
            self.pending_buffer.clear()
            
            output_rows.append(frame.row_data)
            self.last_housed_anchor = frame
        else:
            # Non-overlay frame
            if self.last_housed_anchor:
                if frame.timestamp - self.last_housed_anchor.timestamp > self.timeout_sec:
                    # Window timed out: flush buffer as raw and reset anchor
                    for bf in self.pending_buffer:
                        output_rows.append(bf.row_data)
                    self.pending_buffer.clear()
                    
                    output_rows.append(frame.row_data)
                    self.last_housed_anchor = None
                else:
                    # Within window: add to buffer for potential bridging
                    self.pending_buffer.append(frame)
            else:
                # No active anchor: return immediately
                output_rows.append(frame.row_data)

        return output_rows

    def flush(self) -> List[Dict[str, Any]]:
        """Flushes any remaining frames in the buffer."""
        output_rows = [f.row_data for f in self.pending_buffer]
        self.pending_buffer.clear()
        return output_rows
