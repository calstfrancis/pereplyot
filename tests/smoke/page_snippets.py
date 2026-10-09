#!/usr/bin/env python3
"""Reads a sidecar on stdin and prints "page:snippet" for each annotation, joined by "|"."""
import json, sys

try:
    d = json.load(sys.stdin)
    print("|".join(f"{a.get('page')}:{(a.get('snippet') or '').strip()}" for a in d.get("annotations", [])))
except Exception:
    print("")
