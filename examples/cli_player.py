#!/usr/bin/env python3
import argparse
import json
import shutil
import subprocess
import sys
import threading
import time
from urllib.parse import urlparse

import requests


def sse_listen(url: str, on_data) -> None:
    session = requests.Session()
    session.trust_env = False
    headers = {"Accept": "text/event-stream"}
    while True:
        try:
            # Long read timeout to allow idle SSE; reconnect on timeout
            with session.get(url, stream=True, timeout=(2, 300), headers=headers) as r:
                r.raise_for_status()
                buf = []
                for line in r.iter_lines(decode_unicode=True):
                    if line is None:
                        continue
                    if not line:
                        if buf:
                            data_lines = [l[5:] for l in buf if l.startswith("data:")]
                            if data_lines:
                                payload = "\n".join(data_lines)
                                on_data(payload)
                            buf.clear()
                        continue
                    # ignore SSE comments/keepalives
                    if line.startswith(":"):
                        continue
                    buf.append(line)
        except (requests.exceptions.ReadTimeout, requests.exceptions.ConnectionError):
            time.sleep(1.0)
            continue
        except Exception:
            time.sleep(2.0)
            continue


def pick_player() -> str:
    for p in ("mpv", "ffplay"):
        if shutil.which(p):
            return p
    return ""


def main() -> None:
    ap = argparse.ArgumentParser(description="Shortwave CLI player")
    ap.add_argument("--node", required=True, help="Base URL of any Shortwave node, e.g. http://127.0.0.1:8080")
    ap.add_argument("--frequency", help="Frequency to tune to (decimal as string)")
    ap.add_argument("--player", choices=["mpv", "ffplay", "auto"], default="auto")
    args = ap.parse_args()

    sess = requests.Session()
    sess.trust_env = False

    # Fetch stations directory
    r = sess.get(args.node.rstrip("/") + "/api/v1/stations", timeout=5)
    r.raise_for_status()
    stations = r.json()
    if not stations:
        print("No stations found.")
        sys.exit(2)

    station = None
    if args.frequency:
        for s in stations:
            if str(s.get("frequency")) == args.frequency:
                station = s
                break
        if not station:
            print(f"Frequency {args.frequency} not found.")
            sys.exit(3)
    else:
        print("Available stations:")
        for i, s in enumerate(stations, 1):
            print(f"  {i}) {s.get('frequency')} - {s.get('name')}")
        try:
            idx = int(input("Select #: ").strip())
            station = stations[idx - 1]
        except Exception:
            print("Invalid selection")
            sys.exit(4)

    name = station.get("name")
    stream_url = station.get("stream_url")
    print(f"Tuning to {station.get('frequency')} - {name}")

    # Now-playing SSE from the station's host
    su = urlparse(stream_url)
    station_base = f"{su.scheme}://{su.netloc}"
    now_url = station_base + "/api/v1/now/events"

    def on_now(payload: str):
        try:
            obj = json.loads(payload)
            title = obj.get("title") or ""
            artist = obj.get("artist") or ""
            album = obj.get("album") or ""
            cover = obj.get("cover_url") or ""
            parts = [p for p in [artist, title] if p]
            line = " - ".join(parts) or name
            if album:
                line += f" [{album}]"
            if cover:
                line += f" \n  cover: {cover}"
            print(f"\nNow Playing: {line}")
        except Exception:
            pass

    t = threading.Thread(target=sse_listen, args=(now_url, on_now), daemon=True)
    t.start()

    player = args.player
    if player == "auto":
        player = pick_player()
    if not player:
        print("No supported player found (mpv/ffplay). Install one or specify --player.")
        sys.exit(5)

    try:
        if player == "mpv":
            subprocess.run(["mpv", "--no-video", stream_url], check=False)
        elif player == "ffplay":
            subprocess.run(["ffplay", "-nodisp", "-autoexit", stream_url], check=False)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()


