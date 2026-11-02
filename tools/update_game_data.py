"""Regenerates data/game_data.csv: the units, buildings and technologies a replay can refer
to, with their costs and English names.

IDs and costs come from SiegeEngineers/aoe2techtree (data/data.json, the data behind
aoe2techtree.net); names come from SiegeEngineers/halfon (data/units_buildings_techs.de.json,
`localised_name`). Both are fetched at the pinned commits below, so the output is
reproducible. After a game update, set both commits to the ones that implement it, run this
script, review the diff of data/game_data.csv and update the commits in data/SOURCES.md.

Unit upgrades (Crossbowman, Pikeman, ...) are technologies; aoe2techtree lists them under
`unit_upgrades`, keyed by the unit they upgrade to.

Usage:
    python tools/update_game_data.py [--techtree <commit>] [--halfon <commit>]
"""

import argparse
import csv
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "data" / "game_data.csv"

# DE Update 188329 (both 2026-10-27).
TECHTREE_COMMIT = "e56728e76b7d08301fba7b18d5f16bc8554c109a"
HALFON_COMMIT = "7f96c37fc5f63c7141ec7d88e91c627beb375b28"

TECHTREE_URL = "https://raw.githubusercontent.com/SiegeEngineers/aoe2techtree/{}/data/data.json"
HALFON_URL = (
    "https://raw.githubusercontent.com/SiegeEngineers/halfon/{}/data/units_buildings_techs.de.json"
)

# Display names that differ from Halfon's. Keep this short and explain each entry.
NAME_OVERRIDES = {
    # Halfon names the trainable villager after its male variant; the game shows "Villager".
    ("unit", 83): "Villager",
}

RESOURCES = ("Food", "Wood", "Gold", "Stone")


def fetch_json(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        return json.load(response)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--techtree", default=TECHTREE_COMMIT)
    parser.add_argument("--halfon", default=HALFON_COMMIT)
    args = parser.parse_args()

    techtree = fetch_json(TECHTREE_URL.format(args.techtree))["data"]
    halfon = fetch_json(HALFON_URL.format(args.halfon))

    # (kind, id, cost dict, Halfon section the name comes from)
    entries = []
    for kind, section, halfon_section in (
        ("unit", "Unit", "units_buildings"),
        ("building", "Building", "units_buildings"),
        ("tech", "Tech", "techs"),
    ):
        for key, item in techtree[section].items():
            entries.append((kind, int(key), item["Cost"], halfon_section))
    for item in techtree["unit_upgrades"].values():
        entries.append(("tech", item["ID"], item["Cost"], "techs"))

    rows = {}
    missing = []
    for kind, id_, cost, halfon_section in entries:
        name = NAME_OVERRIDES.get((kind, id_))
        if name is None:
            name = (halfon[halfon_section].get(str(id_)) or {}).get("localised_name")
        if not name:
            missing.append(f"{kind} {id_}")
            continue
        row = [kind, id_, name] + [cost.get(r, 0) for r in RESOURCES]
        if (kind, id_) in rows and rows[(kind, id_)] != row:
            sys.exit(f"conflicting entries for {kind} {id_}: {rows[(kind, id_)]} / {row}")
        rows[(kind, id_)] = row
    if missing:
        sys.exit("no name in Halfon for: " + ", ".join(missing))

    kind_order = {"unit": 0, "building": 1, "tech": 2}
    with OUTPUT.open("w", newline="\n", encoding="utf-8") as f:
        writer = csv.writer(f, lineterminator="\n")
        writer.writerow(["kind", "id", "name", "food", "wood", "gold", "stone"])
        for key in sorted(rows, key=lambda k: (kind_order[k[0]], k[1])):
            writer.writerow(rows[key])

    counts = {k: sum(1 for r in rows if r[0] == k) for k in kind_order}
    print(f"Wrote {OUTPUT.relative_to(ROOT)}: {counts}")
    print(f"aoe2techtree {args.techtree}, halfon {args.halfon}")


if __name__ == "__main__":
    main()
