#!/usr/bin/env python3
"""Rewrite GTK re-exports of glib, gio, pango and gdk_pixbuf in core modules to direct crate paths."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORE = ["crates/strata-core/src"]
DIRECT = {"glib", "gio", "gdk_pixbuf"}
REWRITABLE = DIRECT | {"pango", "prelude"}

GROUP = re.compile(r"^([ \t]*)use gtk::\{([^{}]*)\};[ \t]*\n", re.M)
BARE_CRATE = re.compile(r"^[ \t]*use (?:glib|gio|gdk_pixbuf);[ \t]*\n", re.M)
GTK_PATH = re.compile(r"\bgtk::(\w+)")


def core_files():
    for entry in CORE:
        path = ROOT / entry
        if path.is_file():
            yield path
        elif path.is_dir():
            yield from sorted(path.rglob("*.rs"))


def group_items(body):
    return [item.strip() for item in body.split(",") if item.strip()]


def names_gtk_type(text):
    if any(m.group(1) not in REWRITABLE for m in GTK_PATH.finditer(text)):
        return True
    return any(
        item.split("::")[0] not in REWRITABLE
        for m in GROUP.finditer(text)
        for item in group_items(m.group(2))
    )


def direct_path(rest, keep_prelude):
    head = rest.split("::")[0]
    if head in DIRECT:
        return rest
    if head == "pango":
        return f"pangocairo::{rest}"
    if head == "prelude" and not keep_prelude:
        return f"gio::{rest}"
    return None


def split_group(match, keep_prelude):
    indent, body = match.groups()
    kept, direct = [], []
    for item in group_items(body):
        path = direct_path(item, keep_prelude)
        if path is None:
            kept.append(item)
        elif path not in DIRECT:
            direct.append(path)
    lines = []
    if kept:
        lines.append(f"use gtk::{{{', '.join(kept)}}};")
    lines.extend(f"use {path};" for path in direct)
    return "".join(f"{indent}{line}\n" for line in lines)


def rewrite(text):
    keep_prelude = names_gtk_type(text)
    text = GROUP.sub(lambda m: split_group(m, keep_prelude), text)
    text = GTK_PATH.sub(
        lambda m: direct_path(m.group(0)[5:], keep_prelude) or m.group(0), text
    )
    return BARE_CRATE.sub("", text)


def main():
    for path in core_files():
        original = path.read_text()
        updated = rewrite(original)
        if updated != original:
            path.write_text(updated)
            print(path.relative_to(ROOT))


if __name__ == "__main__":
    main()
