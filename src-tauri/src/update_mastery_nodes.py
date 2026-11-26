"""Refresh bundled amounts: python3 src-tauri/src/update_mastery_nodes.py [missions-data.lua].

Without a local input, fetch https://wiki.warframe.com/w/Module:Missions/data?action=raw.
Only existing table keys are updated; new nodes need their planet and gate reviewed.
"""

import re
import sys
from pathlib import Path
from urllib.request import urlopen


def main():
    if len(sys.argv) > 1:
        data = Path(sys.argv[1]).read_text(encoding="utf-8")
    else:
        with urlopen("https://wiki.warframe.com/w/Module:Missions/data?action=raw", timeout=60) as response:
            data = response.read().decode("utf-8")

    # Mission records occupy one line even when they contain nested Lua tables.
    amounts = {}
    for line in data.splitlines():
        key = re.search(r'\bInternalName\s*=\s*"([^"]+)"', line)
        amount = re.search(r"\bMasteryExp\s*=\s*(\d+)\s*[,}]", line)
        if key and amount:
            key, amount = key[1], int(amount[1])
            if key in amounts and amounts[key] != amount:
                raise ValueError(f"Conflicting mastery amounts for {key}")
            amounts[key] = amount

    table = Path(__file__).with_name("mastery_nodes.rs")
    source = table.read_text(encoding="utf-8")

    def update(match):
        key = match[2]
        if key not in amounts:
            raise ValueError(f"Missing mastery amount for {key}")
        if key.endswith("Junction"):
            if amounts[key] != 1000:
                raise ValueError(f"Unexpected junction mastery for {key}: {amounts[key]}")
            return match[0]
        return f"{match[1]}, {amounts[key]}),"

    updated, count = re.subn(r'^(        \("([^"]+)", "[^"]+")(?:, \d+)?\),$', update, source, flags=re.MULTILINE)
    if count == 0:
        raise ValueError("No bundled nodes matched")
    table.write_text(updated, encoding="utf-8")
    print(f"Updated {count} bundled nodes and junctions")


if __name__ == "__main__":
    main()
