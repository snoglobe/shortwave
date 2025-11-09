#!/usr/bin/env python3
import argparse
import json
import random
import socket
import subprocess
import sys
from pathlib import Path
from typing import Iterator, List, Optional

import requests


def find_files(paths: List[str], exts: List[str]) -> List[str]:
    files: List[str] = []
    for p in paths:
        path = Path(p)
        if path.is_dir():
            for ext in exts:
                files += [str(f) for f in sorted(path.rglob(f"*{ext}"))]
        else:
            files.append(str(path))
    return [f for f in files if Path(f).is_file()]


def ffprobe_tags(path: str) -> dict:
    try:
        cmd = [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format_tags=title,artist,album",
            "-of",
            "json",
            path,
        ]
        out = subprocess.check_output(cmd)
        data = json.loads(out.decode("utf-8"))
        tags = data.get("format", {}).get("tags", {})
        return {k: tags.get(k) for k in ("title", "artist", "album") if tags.get(k)}
    except Exception:
        return {}


def send_ipc(ipc_path: Optional[str], payload: dict) -> None:
    if not ipc_path:
        return
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(ipc_path)
        s.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        s.close()
    except Exception as e:
        print(f"IPC send failed: {e}", file=sys.stderr)


def chunks_generator(
    playlist: List[str], ipc_path: Optional[str], loop: bool, cover_base_url: Optional[str]
) -> Iterator[bytes]:
    index = 0
    while True:
        if index >= len(playlist):
            if not loop:
                return
            index = 0

        path = playlist[index]
        index += 1

        tags = ffprobe_tags(path)
        cover_url = None
        if cover_base_url:
            name = Path(path).stem
            cover_url = cover_base_url.rstrip("/") + "/" + name + ".jpg"

        np = {
            "title": tags.get("title") or Path(path).stem,
            "artist": tags.get("artist"),
            "album": tags.get("album"),
            "cover_url": cover_url,
        }
        send_ipc(ipc_path, np)

        cmd = [
            "ffmpeg",
            "-v",
            "error",
            "-nostdin",
            "-re",
            "-i",
            path,
            "-vn",
            "-ac",
            "2",
            "-ar",
            "44100",
            "-b:a",
            "192k",
            "-f",
            "mp3",
            "-",
        ]
        with subprocess.Popen(cmd, stdout=subprocess.PIPE) as proc:
            assert proc.stdout is not None
            while True:
                chunk = proc.stdout.read(16384)
                if not chunk:
                    break
                yield chunk
            proc.wait()


def main() -> None:
    ap = argparse.ArgumentParser(description="Playlist streamer for Shortwave (audio over Unix socket)")
    ap.add_argument("--ipc", help="Unix socket path to send NowPlaying JSON")
    ap.add_argument("--audio-ipc", required=True, help="Unix socket path to send raw audio bytes")
    ap.add_argument("--loop", action="store_true", help="Loop playlist")
    ap.add_argument("--shuffle", action="store_true", help="Shuffle playlist")
    ap.add_argument("--cover-base-url", help="Optional base URL to derive cover art per-track (name.jpg)")
    ap.add_argument("paths", nargs="+", help="Audio files or directories")
    args = ap.parse_args()

    files = find_files(args.paths, exts=[".mp3", ".flac", ".wav", ".m4a", ".ogg"])    
    if not files:
        print("No audio files found", file=sys.stderr)
        sys.exit(1)
    if args.shuffle:
        random.shuffle(files)

    # Stream raw audio bytes to the Unix socket (required)
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(args.audio_ipc)
        for chunk in chunks_generator(files, args.ipc, args.loop, args.cover_base_url):
            s.sendall(chunk)
        s.close()
        print("Audio IPC stream finished")
    except KeyboardInterrupt:
        print("Interrupted")
    except Exception as e:
        print(f"Audio IPC error: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()


