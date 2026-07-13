#!/usr/bin/env python3
import sys
import tomllib

DOCS = "docs/configuration/config/README.md"


def keys(table, prefix=""):
    for key, value in table.items():
        key = f"{prefix}.{key}" if prefix else key
        if isinstance(value, dict):
            yield from keys(value, key)
        else:
            yield key


docs = open(DOCS, encoding="utf-8").read()
config = tomllib.load(open("default_config.toml", "rb"))
missing = [
    key
    for key in keys(config)
    if f"`{key}`" not in docs and f"`{key.rsplit('.', 1)[-1]}`" not in docs
]

if missing:
    print(f"{DOCS} is missing default_config.toml settings:", file=sys.stderr)
    print("\n".join(f"  - `{key}`" for key in missing), file=sys.stderr)
    sys.exit(1)
