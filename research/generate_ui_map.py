
import json

# ==========================================
# CALIBRATION SETTINGS (Adjust these)
# ==========================================
RED_MARGIN_BASELINE = 263  # Based on aoe2_16x9_max.png
BOX_DISTANCE = 125        # Variable distance between Resource/Vil groups

# Resource Total Box dimensions & Wood start
RES_W = 65
RES_H = 22
RES_Y = 25
WOOD_TOTAL_X = 64
POP_TOTAL_EXTRA_W = 10  # Extra width for population total box

# Villager Box dimensions & Wood start
VIL_W = 37
VIL_H = 16
VIL_Y = 47
WOOD_VIL_X = 22

# Idle Villager Box settings
IDLE_Y = 37
IDLE_OFFSET = 92  # Distance from the population total box

def calculate_ui_map():
    resources = ["wood", "food", "gold", "stone", "population"]
    elements = {}
    
    for i, name in enumerate(resources):
        # 1. Total Count Box
        # Each box starts another BOX_DISTANCE further to the right of Wood
        t_x = WOOD_TOTAL_X + i * BOX_DISTANCE
        t_w = RES_W
        if name == "population":
            t_w += POP_TOTAL_EXTRA_W
            
        elements[f"{name}_total"] = {
            "x_px": t_x,
            "y_px": RES_Y,
            "w_px": t_w,
            "h_px": RES_H,
            "split_pct": 1.0
        }
        
        # 2. Villager Count Box
        # Wood box starts at x=26, each subsequent 126px further
        v_x = WOOD_VIL_X + i * BOX_DISTANCE
        
        elements[f"{name}_vils"] = {
            "x_px": v_x,
            "y_px": VIL_Y,
            "w_px": VIL_W,
            "h_px": VIL_H,
            "split_pct": 1.0
        }
        
    # 3. Idle Villager Box
    # offset to the right from the population total box
    idle_x = (WOOD_TOTAL_X + 4 * BOX_DISTANCE) + IDLE_OFFSET
    elements["idle_vils"] = {
        "x_px": idle_x,
        "y_px": IDLE_Y,
        "w_px": VIL_W,
        "h_px": VIL_H,
        "split_pct": 1.0
    }
        
    return {
        "baseline_margin": RED_MARGIN_BASELINE,
        "elements": elements
    }

def main():
    # 1. Calculate the UI Map
    ui_map = calculate_ui_map()
    
    # 2. Store the calibration in ui_map.json
    with open("ui_map.json", "w") as f:
        json.dump(ui_map, f, indent=4)
    print("Updated ui_map.json with new calibrated coordinates.")

if __name__ == "__main__":
    main()
