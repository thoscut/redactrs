#!/usr/bin/env python3
"""Prüft das gebaute Doku-Buch (scripts/build-doku.sh) auf tote Verweise.

Verwendung: python3 scripts/check-doku.py [verzeichnis]   (Vorgabe: target/doku)

Jeder relative Verweis (`href`, `src`) jeder HTML-Seite muss auf eine Datei im
Buch zeigen, und ein Anker (`#…`) auf eine `id` dieser Seite. Ein Verweis auf
eine Markdown-Datei, die nicht in book/SUMMARY.md steht, wird von mdBook zu
einer `.html`-Datei umgeschrieben, die es nicht gibt - genau das findet diese
Prüfung. Nur Standardbibliothek.
"""

import os
import sys
from html.parser import HTMLParser
from urllib.parse import unquote, urlsplit


class Seite(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.verweise = []

    def handle_starttag(self, tag, attrs):
        for name, wert in attrs:
            if name in ("id", "name") and wert:
                self.ids.add(wert)
            if name in ("href", "src") and wert:
                self.verweise.append(wert)


def lies(pfad):
    seite = Seite()
    with open(pfad, encoding="utf-8") as f:
        seite.feed(f.read())
    return seite


def main():
    wurzel = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/doku")
    if not os.path.isfile(os.path.join(wurzel, "index.html")):
        print(f"Fehler: {wurzel} enthält kein gebautes Buch", file=sys.stderr)
        return 2
    seiten = {}
    for verz, _, dateien in os.walk(wurzel):
        for d in dateien:
            if d.endswith(".html"):
                p = os.path.join(verz, d)
                seiten[p] = lies(p)

    fehler = []
    geprueft = 0
    for pfad, seite in sorted(seiten.items()):
        for verweis in seite.verweise:
            teile = urlsplit(verweis)
            if teile.scheme or teile.netloc or verweis.startswith("javascript:"):
                continue
            geprueft += 1
            ziel = (
                os.path.normpath(os.path.join(os.path.dirname(pfad), unquote(teile.path)))
                if teile.path
                else pfad
            )
            rel = os.path.relpath(pfad, wurzel)
            if not os.path.exists(ziel):
                fehler.append(f"{rel}: {verweis} - Datei fehlt")
                continue
            anker = unquote(teile.fragment)
            if anker and ziel.endswith(".html"):
                ids = seiten[ziel].ids if ziel in seiten else lies(ziel).ids
                if anker not in ids:
                    fehler.append(f"{rel}: {verweis} - Anker fehlt")

    for f in fehler:
        print(f)
    if fehler:
        print(f"{len(fehler)} von {geprueft} Verweisen sind tot.")
        return 1
    print(f"Alle {geprueft} Verweise in {len(seiten)} Seiten treffen.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
