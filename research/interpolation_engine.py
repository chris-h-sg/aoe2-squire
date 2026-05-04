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

INTERPOLATION_TIMEOUT_SEC = 1.0

class InterpolationEngine:
    def __init__(self, timeout_sec: float = INTERPOLATION_TIMEOUT_SEC):
        self.timeout_sec = timeout_sec
        self.pending_buffer: List[CapturedFrame] = []
        self.last_housed_anchor: Optional[CapturedFrame] = None
        self.last_queued_anchor: Optional[CapturedFrame] = None

    def process_frame(self, frame: CapturedFrame) -> List[Dict[str, Any]]:
        """
        Processes a single frame and returns a list of row_data to be written to output.
        Frames may be buffered to allow for state interpolation.
        """
        is_overlay = frame.pop_color == 'overlay'
        is_yellow = frame.pop_color == 'yellow'
        is_anchor = is_overlay or is_yellow
        
        output_rows = []

        if is_anchor:
            # 1. Resolve bridges for buffered frames
            
            # Housed Bridge: overlay -> ... -> overlay
            if is_overlay and self.last_housed_anchor:
                if frame.timestamp - self.last_housed_anchor.timestamp <= self.timeout_sec:
                    max_diff = max(self.last_housed_anchor.pop_diff, frame.pop_diff)
                    for bf in self.pending_buffer:
                        if bf.pop_diff <= max_diff:
                            bf.row_data['housing'] = 'housed'

            # Queued Bridge: (yellow|overlay) -> ... -> (yellow|overlay)
            if self.last_queued_anchor:
                if frame.timestamp - self.last_queued_anchor.timestamp <= self.timeout_sec:
                    max_diff = max(self.last_queued_anchor.pop_diff, frame.pop_diff)
                    for bf in self.pending_buffer:
                        # Only upgrade to queued if not already upgraded to housed
                        if bf.row_data['housing'] == 'normal' and bf.pop_diff <= max_diff:
                            bf.row_data['housing'] = 'queued'

            # 2. Decide whether to flush or continue buffering
            
            if is_overlay:
                # Resolve all buffered frames and flush
                for bf in self.pending_buffer:
                    output_rows.append(bf.row_data)
                self.pending_buffer.clear()
                output_rows.append(frame.row_data)
                
                self.last_housed_anchor = frame
                self.last_queued_anchor = frame
            else: # yellow
                # If a housed bridge is potentially still open, buffer this yellow frame
                if self.last_housed_anchor and (frame.timestamp - self.last_housed_anchor.timestamp <= self.timeout_sec):
                    self.pending_buffer.append(frame)
                    self.last_queued_anchor = frame
                else:
                    # No active housed bridge, resolve up to this yellow anchor and flush
                    for bf in self.pending_buffer:
                        output_rows.append(bf.row_data)
                    self.pending_buffer.clear()
                    output_rows.append(frame.row_data)
                    
                    self.last_housed_anchor = None
                    self.last_queued_anchor = frame
        else:
            # Non-anchor frame (white)
            housed_active = self.last_housed_anchor and (frame.timestamp - self.last_housed_anchor.timestamp <= self.timeout_sec)
            queued_active = self.last_queued_anchor and (frame.timestamp - self.last_queued_anchor.timestamp <= self.timeout_sec)
            
            if housed_active or queued_active:
                self.pending_buffer.append(frame)
            else:
                # Break: flush buffer and current frame
                for bf in self.pending_buffer:
                    output_rows.append(bf.row_data)
                self.pending_buffer.clear()
                output_rows.append(frame.row_data)
                
                self.last_housed_anchor = None
                self.last_queued_anchor = None

        return output_rows

    def flush(self) -> List[Dict[str, Any]]:
        """Flushes any remaining frames in the buffer."""
        output_rows = [f.row_data for f in self.pending_buffer]
        self.pending_buffer.clear()
        return output_rows
