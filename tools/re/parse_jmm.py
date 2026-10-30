#!/usr/bin/env python3
"""Разбор .jmm макроса AJAZZ.

Файл — структура 0x342 байт, побайтово повёрнутая вправо на 2 бита.
После обратного поворота:
  0x00  magic 0xFACB99A0
  0x14  имя UTF-16LE, null-terminated
  0x6A  события по 8 байт: <u16 key><u16 state><u32 delay_ms>
        список заканчивается key == 0
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

MAGIC = 0xFACB99A0
HEADER_LEN = 0x342


def rotate_left2(data: bytes) -> bytes:
    return bytes(((value << 2) & 0xFF) | (value >> 6) for value in data)


def rotate_right2(data: bytes) -> bytes:
    return bytes(((value >> 2) | ((value << 6) & 0xFF)) for value in data)


def known_key(code: int) -> str:
    if 0x41 <= code <= 0x5A:
        return chr(code)
    if 0x61 <= code <= 0x7A:
        return chr(code).upper()
    if 0x30 <= code <= 0x39:
        return chr(code)
    return {
        0x08: "Backspace",
        0x09: "Tab",
        0x0D: "Enter",
        0x10: "Shift",
        0x11: "Ctrl",
        0x12: "Alt",
        0x1B: "Escape",
        0x20: "Space",
        0x25: "Left",
        0x26: "Up",
        0x27: "Right",
        0x28: "Down",
        0xA0: "Left Shift",
        0xA1: "Right Shift",
        0xA2: "Left Ctrl",
        0xA3: "Right Ctrl",
        0xA4: "Left Alt",
        0xA5: "Right Alt",
        0xF0: "Media Play/Pause",
        0xF1: "Media Stop",
        0xF2: "Media Prev",
        0xF3: "Volume Up",
        0xF4: "Media Next",
        0xF5: "Volume Down",
        0xF6: "Mute",
    }.get(code, f"0x{code:04X}")


def parse(path: Path) -> dict:
    raw = path.read_bytes()
    if len(raw) != HEADER_LEN:
        raise ValueError(f"expected {HEADER_LEN} bytes, got {len(raw)}")
    data = rotate_left2(raw)
    magic = int.from_bytes(data[:4], "little")
    if magic != MAGIC:
        raise ValueError(f"bad magic 0x{magic:08X}")

    end = data.find(b"\x00\x00", 0x14)
    if end < 0:
        name = ""
    else:
        name_bytes = data[0x14:end + (end - 0x14) % 2]
        name = name_bytes.decode("utf-16le", errors="replace").rstrip("\x00")

    events = []
    offset = 0x6A
    while offset + 8 <= len(data):
        key, state, delay = struct_unpack(data, offset)
        if key == 0:
            break
        events.append(
            {
                "offset": offset,
                "key": key,
                "key_name": known_key(key),
                "state": state,
                "state_name": "down" if state & 1 else "up",
                "delay_ms": delay,
            }
        )
        offset += 8

    return {
        "file": str(path),
        "encoded_len": len(raw),
        "magic": f"0x{magic:08X}",
        "name": name,
        "header_words": {
            f"0x{offset:02X}": int.from_bytes(data[offset : offset + 4], "little")
            for offset in range(0x04, 0x14, 4)
        },
        "events": events,
        "encoded_hex": raw.hex(),
    }


def struct_unpack(data: bytes, offset: int) -> tuple[int, int, int]:
    key = int.from_bytes(data[offset : offset + 2], "little")
    state = int.from_bytes(data[offset + 2 : offset + 4], "little")
    delay = int.from_bytes(data[offset + 4 : offset + 8], "little")
    return key, state, delay


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=Path)
    parser.add_argument("--json", type=Path, default=None)
    args = parser.parse_args()

    parsed = parse(args.file)
    print(f"File:  {parsed['file']}")
    print(f"Magic: {parsed['magic']}")
    print(f"Name:  {parsed['name']!r}")
    print(f"Header: {parsed['header_words']}")
    print(f"Events: {len(parsed['events'])}")
    for event in parsed["events"]:
        print(
            f"  +0x{event['offset']:03X}: {event['key_name']:<20} "
            f"key=0x{event['key']:04X} state={event['state_name']:<4} "
            f"delay={event['delay_ms']} ms"
        )
    if args.json:
        args.json.write_text(
            json.dumps(parsed, ensure_ascii=False, indent=2), encoding="utf-8"
        )
        print(f"JSON: {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
