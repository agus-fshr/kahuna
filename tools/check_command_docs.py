#!/usr/bin/env python3
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PARSER = ROOT / "libsurfer" / "src" / "command_parser.rs"
DOCS = ROOT / "docs" / "commands" / "README.md"

TOP_LEVEL_MATCH_ARM_RE = re.compile(r'^ {16}(?:"[a-z0-9_]+"\s*(?:\|\s*)?)+\s*=>')
COMMAND_RE = re.compile(r'"([a-z0-9_]+)"')
DOC_CODE_RE = re.compile(r'``([^`]+)``')
COMMAND_NAME_RE = re.compile(r'^[a-z][a-z0-9_]*$')


def extract_parser_commands(text: str) -> set[str]:
    commands: set[str] = set()
    for line in text.splitlines():
        if TOP_LEVEL_MATCH_ARM_RE.match(line):
            commands.update(COMMAND_RE.findall(line))
    return commands


def extract_doc_commands(text: str) -> set[str]:
    commands: set[str] = set()
    for block in DOC_CODE_RE.findall(text):
        first = block.strip().split()[0]
        if COMMAND_NAME_RE.match(first):
            commands.add(first)
    return commands


def main() -> int:
    parser_text = PARSER.read_text(encoding="utf-8")
    docs_text = DOCS.read_text(encoding="utf-8")

    parser_commands = extract_parser_commands(parser_text)
    doc_commands = extract_doc_commands(docs_text)

    missing = sorted(parser_commands - doc_commands)

    if missing:
        print("Commands missing from docs/commands/README.md:")
        for command in missing:
            print(f"- {command}")
        return 1

    print(f"All {len(parser_commands)} command_parser commands are documented.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
