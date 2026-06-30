#!/usr/bin/env python3
"""Layout efficiency matrix for the jcode iOS app.

A single screenshot only measures one content state. Real UI quality means the
layout stays efficient across the *range* of content (empty, short, one tool,
long thread, code-heavy) and devices. This harness:

  1. builds the app once,
  2. for each (scenario, device) cell: starts the mock gateway pre-seeded with
     that scenario, seeds a paired credential, launches, screenshots, and
     scores the screenshot with ui_metrics.py,
  3. prints an aggregate table + a single mean overall score.

That mean is the hill to climb: a layout change is an improvement only if it
raises the mean across the whole matrix, not just one lucky screen.

Usage:
  python3 ui_matrix.py [--devices "iPhone 17"] \
      [--scenarios empty,short,tool,long,code] [--out DIR] [--json]
  python3 ui_matrix.py --baseline-json before.json --candidate-json after.json
"""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
IOS = HERE.parent
BUNDLE = "com.jcode.mobile"
APP = IOS / ".build-ios/Build/Products/Debug-iphonesimulator/JCodeMobile.app"
PORT = 7643
TOKEN = "mocktoken0123456789abcdef"
CRED = ('[{"host":"127.0.0.1","port":7643,"token":"%s","serverName":'
        '"mock-jcode","serverVersion":"mock-0.32.0","pairedAt":770000000}]' % TOKEN)

sys.path.insert(0, str(HERE))
import ui_metrics  # noqa: E402


def sh(cmd, **kw):
    return subprocess.run(cmd, shell=True, text=True, capture_output=True, **kw)


def build_app(device):
    sh("xcodegen generate", cwd=IOS)
    r = sh(
        "xcodebuild build -project JCodeMobile.xcodeproj -scheme JCodeMobile "
        f"-destination 'platform=iOS Simulator,name={device}' "
        "-derivedDataPath .build-ios",
        cwd=IOS,
    )
    if "BUILD SUCCEEDED" not in r.stdout:
        print(r.stdout[-2000:], file=sys.stderr)
        raise SystemExit("build failed")


def boot(device):
    sh(f'xcrun simctl boot "{device}"')
    time.sleep(3)


def start_gateway(scenario):
    sh("pkill -f mock_gateway.py")
    time.sleep(0.4)
    # background process; inherit no pipes so it stays alive
    return subprocess.Popen(
        [sys.executable, str(HERE / "mock_gateway.py"),
         "--port", str(PORT), "--host", "127.0.0.1", "--scenario", scenario],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )


def seed_and_launch(device):
    sh(f'xcrun simctl uninstall "{device}" {BUNDLE}')
    sh(f'xcrun simctl install "{device}" "{APP}"')
    container = sh(
        f'xcrun simctl get_app_container "{device}" {BUNDLE} data'
    ).stdout.strip()
    appsup = Path(container) / "Library/Application Support"
    appsup.mkdir(parents=True, exist_ok=True)
    (appsup / "jcode-servers.json").write_text(CRED + "\n")
    sh(f'xcrun simctl launch "{device}" {BUNDLE}')
    time.sleep(5)


def screenshot(device, path):
    sh(f'xcrun simctl io "{device}" screenshot "{path}"')


def device_scale(device):
    # Pro Max / Plus are 3x; SE/older are 2x; default 3x for 17-class.
    return 3


def run_matrix(devices, scenarios, out_dir):
    out_dir.mkdir(parents=True, exist_ok=True)
    results = []
    first_device = devices[0]
    build_app(first_device)
    for device in devices:
        boot(device)
        scale = device_scale(device)
        for scenario in scenarios:
            gw = start_gateway(scenario)
            try:
                seed_and_launch(device)
                shot = out_dir / f"{slug(device)}__{scenario}.png"
                screenshot(device, str(shot))
                card = ui_metrics.analyze(str(shot), scale=scale)
                results.append({
                    "device": device, "scenario": scenario,
                    "shot": str(shot),
                    "overall": card.overall, "space": card.space,
                    "consistency": card.consistency,
                    "legibility": card.legibility, "rhythm": card.rhythm,
                    "fill_ratio": card.fill_ratio,
                    "dead_zone_frac": card.dead_zone_frac,
                })
            finally:
                gw.terminate()
    sh("pkill -f mock_gateway.py")
    return results


def slug(s):
    return s.replace(" ", "-")


def print_table(results):
    cols = ["device", "scenario", "overall", "space", "fill", "dead"]
    print(f"{'device':16} {'scenario':9} {'ovr':>5} {'spc':>5} {'fill':>5} {'dead':>5}")
    print("-" * 52)
    for r in results:
        print(f"{r['device'][:16]:16} {r['scenario']:9} "
              f"{r['overall']:5.1f} {r['space']:5.1f} "
              f"{r['fill_ratio']:5.2f} {r['dead_zone_frac']:5.2f}")
    print("-" * 52)
    mean = sum(r["overall"] for r in results) / max(1, len(results))
    worst = min(results, key=lambda r: r["overall"]) if results else None
    print(f"MEAN overall: {mean:5.1f}")
    if worst:
        print(f"WORST cell:   {worst['overall']:.1f}  "
              f"({worst['device']} / {worst['scenario']})")
    return mean


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--devices", default="iPhone 17")
    ap.add_argument("--scenarios", default="empty,short,tool,long,code")
    ap.add_argument("--out", default=str(Path(os.environ.get("TMPDIR", "/tmp"))
                                         / "jcode-ui-matrix"))
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--baseline-json")
    ap.add_argument("--candidate-json")
    args = ap.parse_args()

    if args.baseline_json and args.candidate_json:
        a = json.loads(Path(args.baseline_json).read_text())
        b = json.loads(Path(args.candidate_json).read_text())
        am = sum(r["overall"] for r in a) / max(1, len(a))
        bm = sum(r["overall"] for r in b) / max(1, len(b))
        print(f"baseline  mean {am:5.1f}")
        print(f"candidate mean {bm:5.1f}")
        print(f"delta     {'+' if bm >= am else ''}{bm - am:.1f}")
        sys.exit(0 if bm >= am - 0.5 else 1)

    devices = [d.strip() for d in args.devices.split(",") if d.strip()]
    scenarios = [s.strip() for s in args.scenarios.split(",") if s.strip()]
    out_dir = Path(args.out)
    results = run_matrix(devices, scenarios, out_dir)

    if args.json:
        print(json.dumps(results, indent=2))
    else:
        print_table(results)
        print(f"\nscreenshots: {out_dir}")


if __name__ == "__main__":
    main()
