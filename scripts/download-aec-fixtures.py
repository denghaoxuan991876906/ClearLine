#!/usr/bin/env python3
"""Download a tiny official Microsoft AEC-Challenge fixture set.

The repository stores WAV files with Git LFS. This script resolves the public
GitHub LFS pointers and downloads only the requested file IDs into .dev so the
binary audio is not committed.
"""

from __future__ import annotations

import argparse
import csv
import io
import json
import re
import urllib.request
from pathlib import Path

REPO = "microsoft/AEC-Challenge"
RAW_BASE = f"https://raw.githubusercontent.com/{REPO}/main"
LFS_BATCH_URL = f"https://github.com/{REPO}.git/info/lfs/objects/batch"
DEFAULT_OUTPUT = Path(".dev/aec-fixtures/aec-challenge")

KINDS = {
    "farend_speech": "datasets/synthetic/farend_speech/farend_speech_fileid_{fileid}.wav",
    "echo_signal": "datasets/synthetic/echo_signal/echo_fileid_{fileid}.wav",
    "nearend_speech": "datasets/synthetic/nearend_speech/nearend_speech_fileid_{fileid}.wav",
    "nearend_mic_signal": "datasets/synthetic/nearend_mic_signal/nearend_mic_fileid_{fileid}.wav",
}


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()


def read_lfs_pointer(path: str) -> tuple[str, int]:
    text = fetch(f"{RAW_BASE}/{path}").decode("utf-8")
    oid_match = re.search(r"oid sha256:([0-9a-f]+)", text)
    size_match = re.search(r"size (\d+)", text)
    if oid_match is None or size_match is None:
        raise RuntimeError(f"not a Git LFS pointer: {path}")
    return oid_match.group(1), int(size_match.group(1))


def request_lfs_downloads(objects: list[tuple[str, str, int]]) -> dict[str, str]:
    body = {
        "operation": "download",
        "transfers": ["basic"],
        "objects": [{"oid": oid, "size": size} for _, oid, size in objects],
    }
    request = urllib.request.Request(
        LFS_BATCH_URL,
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Content-Type": "application/vnd.git-lfs+json",
            "Accept": "application/vnd.git-lfs+json",
        },
    )
    data = json.loads(fetch_request(request).decode("utf-8"))
    result: dict[str, str] = {}
    for obj in data["objects"]:
        result[obj["oid"]] = obj["actions"]["download"]["href"]
    return result


def fetch_request(request: urllib.request.Request) -> bytes:
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def download_meta(output: Path, fileids: list[int]) -> None:
    text = fetch(f"{RAW_BASE}/datasets/synthetic/meta.csv").decode("utf-8")
    rows = list(csv.DictReader(io.StringIO(text)))
    selected = [row for row in rows if int(row["fileid"]) in set(fileids)]
    output.mkdir(parents=True, exist_ok=True)
    with (output / "meta.csv").open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows(selected)


def download_fileids(output: Path, fileids: list[int]) -> None:
    objects: list[tuple[str, str, int]] = []
    for fileid in fileids:
        for kind, pattern in KINDS.items():
            repo_path = pattern.format(fileid=fileid)
            oid, size = read_lfs_pointer(repo_path)
            relative = Path("synthetic") / kind / Path(repo_path).name
            objects.append((str(relative), oid, size))

    downloads = request_lfs_downloads(objects)
    for relative, oid, expected_size in objects:
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        data = fetch(downloads[oid])
        if len(data) != expected_size:
            raise RuntimeError(f"downloaded {relative} has {len(data)} bytes, expected {expected_size}")
        destination.write_bytes(data)
        print(f"downloaded {destination} ({len(data)} bytes)")

    download_meta(output / "synthetic", fileids)
    print(f"wrote {output / 'synthetic' / 'meta.csv'}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--fileid", type=int, action="append")
    args = parser.parse_args()
    fileids = args.fileid if args.fileid is not None else [0]

    download_fileids(args.output, fileids)


if __name__ == "__main__":
    main()
