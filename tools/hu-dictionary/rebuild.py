#!/usr/bin/env python3
"""Rebuild the pinned Magyar Ispell inputs; --check never changes shipped assets."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "assets/dictionaries/hu_HU"

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--source-dir", type=Path)
    source.add_argument("--download", action="store_true", help="download only the pinned public inputs")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    provenance = json.loads((ASSETS / "sources.json").read_text(encoding='utf-8'))
    scratch = ROOT / "Output/DictionaryVerification"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=scratch) as tmp:
        work = Path(tmp)
        data = work / "data"
        data.mkdir()
        for name in ("hu_HU.aff", "hu_HU.dic", "README_hu_HU.txt"):
            info = provenance["files"][name]
            if args.source_dir:
                content = (args.source_dir / name).read_bytes()
            else:
                request = urllib.request.Request(info["url"], headers={"User-Agent": "DesignCraft-dictionary-rebuild"})
                with urllib.request.urlopen(request, timeout=60) as response:
                    content = response.read(info["bytes"] + 1)
            if len(content) != info["bytes"] or hashlib.sha256(content).hexdigest() != info["sha256"]:
                raise ValueError("Pinned input checksum/size mismatch: " + name)
            (data / name).write_bytes(content)
        for script in ("convert_flags.py", "expand_aliases.py"):
            subprocess.run([sys.executable, str(Path(__file__).with_name(script))], cwd=work, check=True)
        for name in ("hu_HU.aff", "hu_HU.dic"):
            generated = data / "expanded" / name
            if args.check:
                if generated.read_bytes() != (ASSETS / name).read_bytes():
                    raise ValueError("Rebuilt data differs: " + name)
            else:
                shutil.copyfile(generated, ASSETS / name)
            print(name + ": " + hashlib.sha256(generated.read_bytes()).hexdigest())
    print("Pinned dictionary transformation verified." if args.check else "Dictionary regenerated.")

if __name__ == "__main__":
    main()
