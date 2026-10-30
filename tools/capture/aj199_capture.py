#!/usr/bin/env python3
"""Разбор pcap/pcapng, записанных через USBPcap.

Команды:
    summary  — сводка кадров и проверка checksum
    writes   — все команды WriteEEPROM (0x07)
    profile  — восстановить EEPROM по ReadEEPROM (0x08)
    diff-profiles — сравнить два профиля JSON
"""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path
from typing import Any

TSHARK_CANDIDATES = [
    Path(r"C:\Program Files\Wireshark\tshark.exe"),
    Path("tshark"),
]

FIELDS = [
    "frame.number",
    "frame.time_relative",
    "usb.device_address",
    "usb.endpoint_address",
    "usb.transfer_type",
    "usb.data_fragment",
    "usbhid.data",
]


def find_tshark() -> str:
    for candidate in TSHARK_CANDIDATES:
        if candidate.is_file():
            return str(candidate)
        found = shutil.which(str(candidate))
        if found:
            return found
    raise SystemExit("tshark not found; install Wireshark or add it to PATH")


def checksum_ok(report: bytes) -> bool:
    return len(report) == 17 and ((0x55 - sum(report[:16])) & 0xFF) == report[16]


def extract(pcap: Path) -> list[dict[str, Any]]:
    command = [
        find_tshark(),
        "-r",
        str(pcap),
        "-T",
        "fields",
        "-E",
        "separator=\t",
        "-E",
        "occurrence=f",
    ]
    for field in FIELDS:
        command += ["-e", field]
    result = subprocess.run(command, capture_output=True, text=True, check=True)
    events: list[dict[str, Any]] = []
    for line in result.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) < len(FIELDS):
            continue
        frame, time, device, endpoint, transfer, fragment, hid = parts[: len(FIELDS)]
        record: dict[str, Any] = {
            "frame": int(frame),
            "time": float(time),
            "device": int(device) if device else None,
            "endpoint": endpoint,
            "transfer": transfer,
        }
        if fragment:
            data = bytes.fromhex(fragment)
            if len(data) == 17 and data[0] == 0x08:
                record.update(
                    kind="feature",
                    bytes=data,
                    command=data[1],
                    checksum_ok=checksum_ok(data),
                )
        if hid:
            data = bytes.fromhex(hid)
            if len(data) == 17 and data[0] == 0x09:
                record.update(
                    kind="input",
                    bytes=data,
                    command=data[1],
                    checksum_ok=checksum_ok(data),
                )
        if "kind" in record:
            events.append(record)
    return events


def summary(pcap: Path) -> int:
    events = extract(pcap)
    features = [event for event in events if event["kind"] == "feature"]
    inputs = [event for event in events if event["kind"] == "input"]
    print(f"File: {pcap}")
    print(f"Feature reports: {len(features)}")
    print(f"Input reports:   {len(inputs)}")
    print(f"Feature commands: {dict(sorted(Counter(e['command'] for e in features).items()))}")
    print(f"Input commands:   {dict(sorted(Counter(e['command'] for e in inputs).items()))}")
    probes = [
        e for e in features
        if not e["checksum_ok"] and e["command"] == 0x03 and e["bytes"][16] == 0
    ]
    bad_features = [e for e in features if not e["checksum_ok"] and e not in probes]
    bad_inputs = [e for e in inputs if not e["checksum_ok"]]
    print(f"FindDev probe reports (checksum 0): {len(probes)}")
    print(f"Bad feature checksums: {len(bad_features)}")
    print(f"Bad input checksums:   {len(bad_inputs)}")
    for event in (bad_features + bad_inputs)[:10]:
        print(f"  frame {event['frame']} cmd=0x{event['command']:02x} {event['bytes'].hex(' ')}")
    return 0


def writes(pcap: Path) -> int:
    events = extract(pcap)
    count = 0
    for event in events:
        if event["kind"] != "feature" or event["command"] != 0x07:
            continue
        data = event["bytes"]
        address = (data[3] << 8) | data[4]
        length = data[5]
        payload = data[6 : 6 + length]
        print(
            f"frame={event['frame']} t={event['time']:.6f} "
            f"addr=0x{address:04x} len={length} data={payload.hex(' ')} "
            f"chk={data[16]:02x} ok={checksum_ok(data)}"
        )
        count += 1
    print(f"WriteEEPROM frames: {count}")
    return 0


def decode_dpi_stage(raw: bytes) -> dict[str, int]:
    hw_x = raw[0] | ((raw[2] & 0x0C) << 6)
    hw_y = raw[1] | ((raw[2] & 0xC0) << 2)
    mul = raw[2] & 0x03
    factor = 100 if (mul & 1) else 50
    return {
        "hw_x": hw_x,
        "hw_y": hw_y,
        "mul": mul,
        "dpi_x": (hw_x + 1) * factor,
        "dpi_y": (hw_y + 1) * factor,
        "checksum_ok": ((0x55 - raw[2] - raw[1] - raw[0]) & 0xFF) == raw[3],
        "raw": int.from_bytes(raw, "little"),
    }


def profile(pcap: Path, out: Path | None) -> int:
    events = extract(pcap)
    memory: dict[int, int] = {}
    reads: list[dict[str, Any]] = []
    index = 0
    while index < len(events):
        event = events[index]
        if event["kind"] == "feature" and event["command"] == 0x08:
            data = event["bytes"]
            address = (data[3] << 8) | data[4]
            length = data[5]
            cursor = index + 1
            while cursor < len(events):
                response = events[cursor]
                if response["kind"] == "input" and response["command"] == 0x08:
                    payload = response["bytes"][6 : 6 + length]
                    for offset, value in enumerate(payload):
                        memory[address + offset] = value
                    reads.append(
                        {
                            "frame": event["frame"],
                            "time": event["time"],
                            "address": address,
                            "length": length,
                            "data": payload.hex(),
                        }
                    )
                    index = cursor + 1
                    break
                cursor += 1
            else:
                break
            continue
        index += 1

    stage_count = memory.get(0x02, 0)
    stages = []
    for stage in range(8):
        start = 0x0C + stage * 4
        raw = bytes(memory.get(start + i, 0) for i in range(4))
        stages.append(decode_dpi_stage(raw))
    colors = []
    for stage in range(8):
        start = 0x2C + stage * 4
        raw = bytes(memory.get(start + i, 0) for i in range(4))
        colors.append(
            {
                "r": raw[0],
                "g": raw[1],
                "b": raw[2],
                "checksum_ok": ((0x55 - raw[0] - raw[1] - raw[2]) & 0xFF) == raw[3],
            }
        )
    keys = []
    for slot in range(16):
        start = 0x60 + slot * 4
        raw = bytes(memory.get(start + i, 0) for i in range(4))
        keys.append(
            {
                "code": raw[0] | (raw[1] << 8) | (raw[2] << 16),
                "checksum_ok": ((0x55 - raw[0] - raw[1] - raw[2]) & 0xFF) == raw[3],
            }
        )

    result = {
        "file": str(pcap),
        "reads": len(reads),
        "polling_code": memory.get(0x00),
        "polling_hz": {1: 1000, 2: 500, 4: 250, 8: 125}.get(memory.get(0x00, 0)),
        "dpi_stage_count": stage_count,
        "current_dpi_stage": memory.get(0x04),
        "dpi_stages": stages,
        "dpi_colors": colors,
        "keys": keys,
        "debounce": memory.get(0xA9),
        "lod_candidate": memory.get(0x0A),
        "sleep_candidate": memory.get(0xAD),
        "params_raw": {
            f"0x{address:02x}": memory.get(address)
            for address in (0x0A, 0x4C, 0x54, 0xA0, 0xA9, 0xAB, 0xAD, 0xAF, 0xB1, 0xB3, 0xB5, 0xB7)
        },
        "raw": "".join(f"{memory.get(address, 0):02x}" for address in range(0x00, 0xB8)),
        "read_requests": reads,
    }

    print(json.dumps({k: v for k, v in result.items() if k not in ("read_requests",)}, ensure_ascii=False, indent=2))
    if out:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"Saved: {out}")
    return 0


def diff_profiles(first: Path, second: Path) -> int:
    left = json.loads(first.read_text(encoding="utf-8"))
    right = json.loads(second.read_text(encoding="utf-8"))
    keys = sorted(set(left) | set(right))
    differences = 0
    for key in keys:
        if key in ("file", "read_requests"):
            continue
        if left.get(key) != right.get(key):
            differences += 1
            print(f"DIFF {key}:")
            print(f"  {first.name}: {left.get(key)}")
            print(f"  {second.name}: {right.get(key)}")
    print(f"Differences: {differences}")
    return 0 if differences == 0 else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["summary", "writes", "profile", "diff-profiles"])
    parser.add_argument("pcap", type=Path, nargs="?")
    parser.add_argument("other", type=Path, nargs="?")
    parser.add_argument("--json", type=Path, default=None)
    args = parser.parse_args()

    if args.command == "diff-profiles":
        if not args.pcap or not args.other:
            parser.error("diff-profiles requires two profile JSON files")
        return diff_profiles(args.pcap, args.other)

    if not args.pcap:
        parser.error("pcap path is required")
    if args.command == "summary":
        return summary(args.pcap)
    if args.command == "writes":
        return writes(args.pcap)
    return profile(args.pcap, args.json)


if __name__ == "__main__":
    sys.exit(main())
