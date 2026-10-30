#!/usr/bin/env python3
# pyright: reportAttributeAccessIssue=false, reportOptionalMemberAccess=false, reportOptionalOperand=false
"""Инвентаризация PE-файла: секции, импорты, RTTI, строки и ссылки на них.

Результат предназначен для ручного анализа и не входит в поставку приложения.
"""
from __future__ import annotations

import argparse
import json
import re
import struct
from pathlib import Path
from typing import Any

import pefile

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_PE = REPO_ROOT / "local" / "original" / "OemDrv.exe"
DEFAULT_OUT = REPO_ROOT / "local" / "re" / "pe_inventory.json"

ASCII_RE = re.compile(rb"[\x20-\x7e]{4,}")
UTF16_RE = re.compile(rb"(?:[\x20-\x7e]\x00){4,}")
RTTI_RE = re.compile(rb"\.\?A[VU][^\x00]{2,200}@@")
RTTI_TEXT_RE = re.compile(r"\.\?A[VU].+@@")


def section_for_offset(pe: pefile.PE, offset: int):
    for section in pe.sections:
        start = section.PointerToRawData
        end = start + section.SizeOfRawData
        if start <= offset < end:
            return section
    return None


def offset_to_rva(pe: pefile.PE, offset: int) -> int | None:
    section = section_for_offset(pe, offset)
    if section is None:
        return None
    return int(section.VirtualAddress + (offset - section.PointerToRawData))


def offset_to_va(pe: pefile.PE, offset: int) -> int | None:
    rva = offset_to_rva(pe, offset)
    if rva is None:
        return None
    optional_header: Any = pe.OPTIONAL_HEADER
    return int(optional_header.ImageBase + rva)


def read_imports(pe: pefile.PE) -> list[dict]:
    imports = []
    if not hasattr(pe, "DIRECTORY_ENTRY_IMPORT"):
        return imports
    for entry in pe.DIRECTORY_ENTRY_IMPORT:
        imports.append(
            {
                "dll": entry.dll.decode(errors="replace"),
                "functions": [
                    item.name.decode(errors="replace") if item.name else f"ordinal:{item.ordinal}"
                    for item in entry.imports
                ],
            }
        )
    return imports


def scan_strings(pe: pefile.PE, data: bytes) -> list[dict]:
    result = []
    seen = set()

    def add(offset: int, text: str, encoding: str) -> None:
        if not text or (offset, text, encoding) in seen:
            return
        seen.add((offset, text, encoding))
        section = section_for_offset(pe, offset)
        result.append(
            {
                "offset": offset,
                "rva": offset_to_rva(pe, offset),
                "va": offset_to_va(pe, offset),
                "section": section.Name.rstrip(b"\x00").decode(errors="replace") if section else None,
                "encoding": encoding,
                "text": text,
            }
        )

    for match in ASCII_RE.finditer(data):
        add(match.start(), match.group().decode("latin-1", errors="replace"), "ascii")

    for match in UTF16_RE.finditer(data):
        raw = match.group()
        add(match.start(), raw.decode("utf-16le", errors="replace").rstrip("\x00"), "utf-16le")

    for match in RTTI_RE.finditer(data):
        add(match.start(), match.group().decode("latin-1", errors="replace"), "rtti-type-descriptor")

    result.sort(key=lambda item: item["offset"])
    return result


def find_xrefs(pe: pefile.PE, data: bytes, strings: list[dict]) -> dict[int, list[int]]:
    text_section = next(
        (s for s in pe.sections if s.Name.rstrip(b"\x00").decode(errors="replace") == ".text"),
        None,
    )
    if text_section is None:
        return {}

    start = int(text_section.PointerToRawData)
    end = start + int(text_section.SizeOfRawData)
    text = data[start:end]
    optional_header: Any = pe.OPTIONAL_HEADER
    text_va = int(optional_header.ImageBase + text_section.VirtualAddress)

    refs: dict[int, list[int]] = {}
    for item in strings:
        va = item.get("va")
        if not va:
            continue
        needle = struct.pack("<I", va)
        positions = []
        cursor = 0
        while True:
            position = text.find(needle, cursor)
            if position < 0:
                break
            positions.append(text_va + position)
            cursor = position + 1
        if positions:
            refs[item["offset"]] = positions
    return refs


def find_rtti_classes(strings: list[dict]) -> list[dict]:
    classes = []
    for item in strings:
        if item["encoding"] != "rtti-type-descriptor":
            continue
        text = item["text"]
        match = RTTI_TEXT_RE.fullmatch(text)
        if not match:
            continue
        name = text[4:].split("@@", 1)[0]
        classes.append({"name": name, "mangled": text, "va": item["va"], "offset": item["offset"]})
    return classes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pe", type=Path, default=DEFAULT_PE)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--filter", default=None, help="regex для фильтра строк в отчёте консоли")
    args = parser.parse_args()

    if not args.pe.exists():
        parser.error(f"PE not found: {args.pe}")

    data = args.pe.read_bytes()
    pe = pefile.PE(data=data, fast_load=False)
    pe.parse_data_directories(
        directories=[
            pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"],
            pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXPORT"],
            pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"],
        ]
    )

    strings = scan_strings(pe, data)
    refs = find_xrefs(pe, data, strings)
    for item in strings:
        item["xrefs"] = refs.get(item["offset"], [])

    file_header: Any = pe.FILE_HEADER
    optional_header: Any = pe.OPTIONAL_HEADER
    inventory = {
        "file": str(args.pe),
        "metadata": {
            "machine": hex(file_header.Machine),
            "timestamp": file_header.TimeDateStamp,
            "image_base": hex(optional_header.ImageBase),
            "entry_point": hex(optional_header.AddressOfEntryPoint),
            "size_of_image": hex(optional_header.SizeOfImage),
        },
        "sections": [
            {
                "name": section.Name.rstrip(b"\x00").decode(errors="replace"),
                "virtual_address": hex(section.VirtualAddress),
                "virtual_size": hex(section.Misc_VirtualSize),
                "raw_offset": hex(section.PointerToRawData),
                "raw_size": hex(section.SizeOfRawData),
                "characteristics": hex(section.Characteristics),
            }
            for section in pe.sections
        ],
        "imports": read_imports(pe),
        "rtti_classes": find_rtti_classes(strings),
        "strings": strings,
    }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(inventory, ensure_ascii=False, indent=2), encoding="utf-8")

    pattern = re.compile(args.filter, re.IGNORECASE) if args.filter else None
    print(f"PE: {args.pe}")
    print(f"Strings: {len(strings)}; with xrefs: {sum(1 for s in strings if s['xrefs'])}")
    important = {"CUSBDriver", "CMouseManager", "CMacroDlg", "CPickMacroDlg", "CRetParam"}
    selected = [c["name"] for c in inventory["rtti_classes"] if c["name"] in important]
    print(f"RTTI classes: {', '.join(selected) if selected else 'none'} (всего {len(inventory['rtti_classes'])})")
    print(f"Output: {args.out}")
    if pattern:
        print("\nMatched strings:")
        for item in strings:
            if pattern.search(item["text"]):
                xrefs = ", ".join(hex(x) for x in item["xrefs"][:6])
                print(f"  {item['va'] and hex(item['va'])} {item['text']!r} xrefs=[{xrefs}]")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
