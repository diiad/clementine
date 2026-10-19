"""Write the updater feed for one platform (dynamic-server format).

usage: update-feed.py <out.json> <asset-url> <signature-file>
The version comes from app/src-tauri/tauri.conf.json.
"""
import datetime
import json
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parent.parent
out, url, sig = sys.argv[1:4]
version = json.loads((root / "app/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["version"]
feed = {
    "version": version,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "url": url,
    "signature": pathlib.Path(sig).read_text(encoding="utf-8").strip(),
    "notes": f"https://github.com/diiad/clementine/releases/tag/v{version}",
}
pathlib.Path(out).write_text(json.dumps(feed, indent=2), encoding="utf-8")
print(f"{out}: {version} -> {url}")
