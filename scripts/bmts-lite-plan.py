"""Turn BMTS Lite's audio-led planner into editable SharbCut decisions.

This deliberately skips BMTS Lite's video render; SharbCut owns the timeline.
"""

import argparse
import json
import math
import sys
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--lite-root", type=Path, required=True)
    parser.add_argument("--bgm", type=Path, required=True)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--duration", type=float, required=True)
    parser.add_argument("--cache-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if not args.lite_root.joinpath("worker.py").is_file():
        parser.error("BMTS Lite source is missing")
    if not args.bgm.is_file() or not args.library.is_dir():
        parser.error("BGM and footage folder must exist")
    if not math.isfinite(args.duration) or args.duration < 5:
        parser.error("duration must be at least five seconds")

    sys.path.insert(0, str(args.lite_root))
    import worker
    from analyze_bgm import analyze_bgm
    from timeline_planner import plan_timeline_slots

    bgm = args.bgm.resolve()
    bgm_duration = worker.seconds(worker.probe(bgm))
    duration = min(bgm_duration, args.duration)
    if duration < 5:
        parser.error("BGM is shorter than five seconds")
    args.cache_dir.mkdir(parents=True, exist_ok=True)
    audio = analyze_bgm(
        bgm,
        args.cache_dir / "audio",
        args.cache_dir / "audiomap.json",
        target_duration=duration,
    )
    slots = plan_timeline_slots(audio, duration)["slots"]
    extensions = {".mp4", ".mov", ".m4v", ".mkv", ".webm", ".avi", ".wmv", ".flv", ".mts", ".m2ts", ".ts"}
    files = sorted(path for path in args.library.rglob("*") if path.is_file() and path.suffix.lower() in extensions)
    if not files:
        parser.error("footage folder has no videos")
    # ponytail: probe at most 24 evenly-spaced videos; add an explicit library picker if selection quality needs finer control.
    files = [files[index * len(files) // min(len(files), 24)] for index in range(min(len(files), 24))]
    media = []
    for path in files:
        try:
            info = worker.probe(path)
            if any(stream.get("codec_type") == "video" for stream in info["streams"]):
                length = worker.seconds(info)
                if length >= 0.5:
                    media.append({"path": str(path.resolve()), "duration": length})
        except (KeyError, ValueError):
            continue
    if not media:
        parser.error("footage folder has no readable videos")

    plan = worker.assign(slots, media, duration, time.time_ns())
    decisions = {
        "schema_version": "sharbcut-bmts-lite-1",
        "shots": plan["shots"],
        "audio_tracks": [{"role": "bgm", "source_path": str(bgm), "timeline_start": 0, "duration": duration}],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(decisions, ensure_ascii=False), encoding="utf-8")
    print(args.output)


if __name__ == "__main__":
    main()
