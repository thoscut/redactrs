#!/usr/bin/env python3
"""Prueft die Belege unter docs/ - unabhaengig vom Programm, das sie erzeugt hat.

    python3 scripts/check-preview.py [verzeichnis]      (Vorgabe: docs)

WARUM PYTHON UND NICHT RUST

Die Bilder entstehen aus einem selbstgeschriebenen PNG- und GIF-Schreiber
(crates/redact-render/examples/common/). Sie mit demselben Code wieder
einzulesen waere ein Zirkelschluss: ein Fehler im Schreiber steckte dann auch
im Leser und faende sich selbst nicht. Dieser Pruefer ist deshalb eine zweite,
unabhaengige Umsetzung - eigener PNG-Leser ueber zlib, eigener
GIF-LZW-Dekodierer - in einer anderen Sprache. Er braucht nichts ausser der
Standardbibliothek.

WARUM NICHT AUF BYTEGLEICHHEIT GEPRUEFT WIRD

Es waere schoener, in der CI einfach `make-preview.sh` laufen zu lassen und
`git diff --exit-code` zu verlangen. Auf derselben Maschine haelt das auch:
vier vollstaendige Laeufe haben hier bytegleiche Dateien geliefert. Ueber
Maschinengrenzen laesst es sich aber nicht zusagen:

  * `tiny-skia` rastert mit SIMD (Feature `simd`). Auf x86_64 laufen SSE-Pfade,
    auf aarch64 NEON-Pfade; bei kantengeglaetteten Raendern kann daraus ein
    Grauwert Unterschied werden.
  * Fliesskomma-Codegen darf sich zwischen `rustc`-Fassungen aendern. Die
    Toolchain ist zwar in rust-toolchain.toml festgenagelt, aber eine
    Anhebung dort aendert dann eben die Bilder.
  * Die PNG-Kompression kommt aus flate2/miniz_oxide ueber Cargo.lock. Eine
    Aktualisierung der Sperrdatei aendert die PNG-Bytes, ohne dass ein Pixel
    anders waere.

Eine CI-Pruefung, die daran gelegentlich scheitert, waere schlimmer als keine:
sie wuerde abgeschaltet. Geprueft wird deshalb, was tatsaechlich zugesagt
werden kann - dass die Dateien entstehen, die richtige Form haben, kein
Einzelbild leer ist und die mitgeschnittenen Rueckgabewerte stimmen.

Was das NICHT leistet: es merkt nicht, wenn die Bilder im Repository veralten,
weil jemand den Rasterizer geaendert und `make-preview.sh` nicht laufen lassen
hat. Dagegen hilft nur der Lauf selbst - `git status` sagt es danach.
"""

import struct
import sys
import zlib
from pathlib import Path

# --------------------------------------------------------------------------
# Erwartungen
#
# Absichtlich Baender und Verhaeltnisse statt exakter Zahlen: eine Pruefung,
# die bei jeder Aenderung an der Aufloesung rot wird, wird abgeschaltet. Was
# hier steht, muss gelten, damit ein Bild ueberhaupt ein Beleg ist.
# --------------------------------------------------------------------------

DATEIEN = ["vorher.png", "nachher.png", "schwaerzung.gif",
           "konsole.gif", "konsole.txt", "pruefung.txt"]

BREITE_MIN, BREITE_MAX = 400, 1600
TINTE_MIN, TINTE_MAX = 0.005, 0.60      # Anteil nicht-Hintergrund je Standbild
GIF_A_BILDER_MIN = 4                    # leer + drei Schwaerzungen
GIF_B_BILDER_MIN = 20
HALT_MIN_CS = 300                       # Standzeit des letzten Einzelbildes
GROESSE_MAX = 500 * 1024                # je Datei

fehler = []
hinweise = []


def pruefe(bedingung, text):
    if bedingung:
        hinweise.append("  ok    " + text)
    else:
        fehler.append("  FEHLT " + text)


# --------------------------------------------------------------------------
# PNG
# --------------------------------------------------------------------------

def png_lesen(pfad):
    daten = pfad.read_bytes()
    if daten[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("keine PNG-Signatur")
    off, idat, ihdr = 8, b"", None
    while off < len(daten):
        (laenge,) = struct.unpack(">I", daten[off:off + 4])
        art = daten[off + 4:off + 8]
        inhalt = daten[off + 8:off + 8 + laenge]
        (crc,) = struct.unpack(">I", daten[off + 8 + laenge:off + 12 + laenge])
        if zlib.crc32(art + inhalt) & 0xFFFFFFFF != crc:
            raise ValueError("CRC-Fehler im Block %s" % art.decode("ascii", "replace"))
        if art == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", inhalt)
        elif art == b"IDAT":
            idat += inhalt
        elif art == b"tIME" or art == b"tEXt":
            raise ValueError("Zusatzblock %s - der Schreiber darf keinen erzeugen"
                             % art.decode("ascii", "replace"))
        off += 12 + laenge
    if ihdr is None:
        raise ValueError("kein IHDR")
    breite, hoehe, tiefe, farbtyp = ihdr[0], ihdr[1], ihdr[2], ihdr[3]
    if tiefe != 8 or farbtyp not in (0, 2):
        raise ValueError("erwartet 8 Bit Graustufen oder RGB, gefunden %d/%d" % (tiefe, farbtyp))
    kanaele = 1 if farbtyp == 0 else 3
    roh = zlib.decompress(idat)
    stride = breite * kanaele
    if len(roh) != (stride + 1) * hoehe:
        raise ValueError("IDAT hat %d statt %d Byte" % (len(roh), (stride + 1) * hoehe))
    # Zeilenfilter zuruecknehmen - sonst misst der Tintenanteil Unsinn.
    aus = bytearray()
    vorher = bytearray(stride)
    pos = 0
    for _ in range(hoehe):
        filt = roh[pos]
        zeile = bytearray(roh[pos + 1:pos + 1 + stride])
        pos += 1 + stride
        for i in range(stride):
            a = zeile[i - kanaele] if i >= kanaele else 0
            b = vorher[i]
            c = vorher[i - kanaele] if i >= kanaele else 0
            if filt == 0:
                add = 0
            elif filt == 1:
                add = a
            elif filt == 2:
                add = b
            elif filt == 3:
                add = (a + b) // 2
            elif filt == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                add = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
            else:
                raise ValueError("unbekannter Zeilenfilter %d" % filt)
            zeile[i] = (zeile[i] + add) & 0xFF
        aus += zeile
        vorher = zeile
    return breite, hoehe, kanaele, bytes(aus)


def png_tinte(breite, hoehe, kanaele, pixel):
    """Anteil der Pixel, die nicht reinweiss sind."""
    treffer = 0
    for i in range(0, len(pixel), kanaele):
        if pixel[i:i + kanaele] != b"\xff" * kanaele:
            treffer += 1
    return treffer / float(breite * hoehe)


# --------------------------------------------------------------------------
# GIF
# --------------------------------------------------------------------------

def gif_lesen(pfad):
    d = pfad.read_bytes()
    if d[:6] != b"GIF89a":
        raise ValueError("kein GIF89a")
    breite, hoehe, packed, _bg, _ar = struct.unpack("<HHBBB", d[6:13])
    off = 13
    palette = []
    if packed & 0x80:
        n = 2 << (packed & 7)
        palette = [tuple(d[off + i * 3:off + i * 3 + 3]) for i in range(n)]
        off += n * 3

    def bloecke_lesen(o):
        raus = bytearray()
        while d[o]:
            raus += d[o + 1:o + 1 + d[o]]
            o += 1 + d[o]
        return bytes(raus), o + 1

    def bloecke_ueberspringen(o):
        while d[o]:
            o += 1 + d[o]
        return o + 1

    bilder, schleife, offen = [], None, None
    while off < len(d):
        marke = d[off]
        if marke == 0x3B:
            break
        if marke == 0x21:
            kennung = d[off + 1]
            if kennung == 0xF9:
                p2, verzug, ti = struct.unpack("<BHB", d[off + 3:off + 7])
                offen = dict(delay=verzug, disposal=(p2 >> 2) & 7,
                             transparent=bool(p2 & 1), ti=ti)
                off = bloecke_ueberspringen(off + 2)
            elif kennung == 0xFF:
                n = d[off + 2]
                if d[off + 3:off + 3 + n].startswith(b"NETSCAPE"):
                    (schleife,) = struct.unpack("<H", d[off + 5 + n:off + 7 + n])
                off = bloecke_ueberspringen(off + 3 + n)
            else:
                off = bloecke_ueberspringen(off + 2)
            continue
        if marke == 0x2C:
            x, y, bw, bh, p2 = struct.unpack("<HHHHB", d[off + 1:off + 10])
            if p2 & 0x80:
                raise ValueError("lokale Farbtabelle - der Schreiber erzeugt keine")
            mcs = d[off + 10]
            nutzlast, off = bloecke_lesen(off + 11)
            pixel = lzw_aus(nutzlast, mcs, bw * bh)
            bild = dict(x=x, y=y, w=bw, h=bh, pixels=pixel)
            bild.update(offen or dict(delay=0, disposal=0, transparent=False, ti=0))
            bilder.append(bild)
            offen = None
            continue
        raise ValueError("unbekannter Block 0x%02X an Stelle %d" % (marke, off))
    return dict(width=breite, height=hoehe, palette=palette,
                frames=bilder, loop=schleife)


def lzw_aus(daten, min_code_size, erwartet):
    clear = 1 << min_code_size
    eoi = clear + 1

    def frisch():
        return [bytes([i]) for i in range(clear)] + [b"", b""]

    tabelle = frisch()
    breite = min_code_size + 1
    voriges = None
    acc = used = pos = 0
    raus = bytearray()
    while True:
        while used < breite and pos < len(daten):
            acc |= daten[pos] << used
            used += 8
            pos += 1
        if used < breite:
            break
        code = acc & ((1 << breite) - 1)
        acc >>= breite
        used -= breite
        if code == clear:
            tabelle, breite, voriges = frisch(), min_code_size + 1, None
            continue
        if code == eoi:
            break
        if code < len(tabelle):
            eintrag = tabelle[code]
        elif voriges is not None:
            eintrag = voriges + voriges[:1]
        else:
            raise ValueError("kaputter LZW-Strom")
        raus += eintrag
        if voriges is not None:
            tabelle.append(voriges + eintrag[:1])
            if len(tabelle) == (1 << breite) and breite < 12:
                breite += 1
        voriges = eintrag
    if len(raus) != erwartet:
        raise ValueError("LZW lieferte %d statt %d Pixel" % (len(raus), erwartet))
    return bytes(raus)


def gif_staende(gif):
    """Tintenanteil nach jedem Einzelbild, uebereinandergelegt."""
    breite, hoehe = gif["width"], gif["height"]
    if not gif["frames"]:
        raise ValueError("keine Einzelbilder")
    leinwand = [0] * (breite * hoehe)
    hintergrund = None
    anteile = []
    for nummer, f in enumerate(gif["frames"]):
        if f["disposal"] not in (0, 1):
            raise ValueError("Einzelbild %d hat Entsorgungsart %d; erwartet 0 oder 1"
                             % (nummer, f["disposal"]))
        if f["x"] + f["w"] > breite or f["y"] + f["h"] > hoehe:
            raise ValueError("Einzelbild %d ragt aus dem Bild" % nummer)
        for yy in range(f["h"]):
            for xx in range(f["w"]):
                leinwand[(f["y"] + yy) * breite + f["x"] + xx] = f["pixels"][yy * f["w"] + xx]
        if hintergrund is None:
            # Der haeufigste Farbindex des ersten Standes ist der Hintergrund.
            zaehler = {}
            for wert in leinwand:
                zaehler[wert] = zaehler.get(wert, 0) + 1
            hintergrund = max(zaehler.items(), key=lambda kv: kv[1])[0]
        anteile.append(sum(1 for wert in leinwand if wert != hintergrund)
                       / float(breite * hoehe))
    return anteile


# --------------------------------------------------------------------------
# Die Pruefungen
# --------------------------------------------------------------------------

def main():
    ordner = Path(sys.argv[1] if len(sys.argv) > 1 else "docs")
    print("Pruefe Belege in %s" % ordner)

    for name in DATEIEN:
        pfad = ordner / name
        pruefe(pfad.is_file() and pfad.stat().st_size > 0, "%s ist vorhanden" % name)
        if pfad.is_file():
            pruefe(pfad.stat().st_size <= GROESSE_MAX,
                   "%s bleibt unter %d kB (%d Byte)"
                   % (name, GROESSE_MAX // 1024, pfad.stat().st_size))
    if fehler:
        return ende()

    # --- Standbilder ------------------------------------------------------
    masse = {}
    for name in ("vorher.png", "nachher.png"):
        breite, hoehe, kanaele, pixel = png_lesen(ordner / name)
        masse[name] = (breite, hoehe)
        anteil = png_tinte(breite, hoehe, kanaele, pixel)
        pruefe(BREITE_MIN <= breite <= BREITE_MAX,
               "%s ist %dx%d px" % (name, breite, hoehe))
        pruefe(TINTE_MIN <= anteil <= TINTE_MAX,
               "%s zeigt etwas (%.2f %% nicht weiss)" % (name, anteil * 100))
    pruefe(masse["vorher.png"] == masse["nachher.png"],
           "vorher.png und nachher.png haben denselben Ausschnitt")

    # nachher.png muss mehr Schwarz tragen als vorher.png - die Balken.
    _, _, kv, pv = png_lesen(ordner / "vorher.png")
    _, _, kn, pn = png_lesen(ordner / "nachher.png")
    schwarz_v = sum(1 for i in range(0, len(pv), kv) if pv[i] < 32)
    schwarz_n = sum(1 for i in range(0, len(pn), kn) if pn[i] < 32)
    pruefe(schwarz_n > schwarz_v,
           "nachher.png hat mehr Schwarz als vorher.png (%d > %d Pixel)"
           % (schwarz_n, schwarz_v))

    # --- Animation A ------------------------------------------------------
    a = gif_lesen(ordner / "schwaerzung.gif")
    pruefe(len(a["frames"]) >= GIF_A_BILDER_MIN,
           "schwaerzung.gif hat %d Einzelbilder" % len(a["frames"]))
    pruefe(a["loop"] == 0, "schwaerzung.gif laeuft in Schleife")
    pruefe(a["frames"][-1]["delay"] >= HALT_MIN_CS,
           "schwaerzung.gif haelt am Schluss %.1f s an"
           % (a["frames"][-1]["delay"] / 100.0))
    staende_a = gif_staende(a)
    pruefe(all(wert > 0 for wert in staende_a),
           "kein Einzelbild von schwaerzung.gif ist leer")
    # Der Kern der Animation: jeder Schritt schwaerzt mehr als der vorige.
    waechst = all(b > a_ for a_, b in zip(staende_a, staende_a[1:]))
    pruefe(waechst,
           "jeder Schritt von schwaerzung.gif zeigt mehr Schwaerzung als der vorige (%s)"
           % " < ".join("%.2f%%" % (wert * 100) for wert in staende_a))

    # --- Animation B ------------------------------------------------------
    b = gif_lesen(ordner / "konsole.gif")
    pruefe(len(b["frames"]) >= GIF_B_BILDER_MIN,
           "konsole.gif hat %d Einzelbilder" % len(b["frames"]))
    pruefe(b["loop"] == 0, "konsole.gif laeuft in Schleife")
    pruefe(b["frames"][-1]["delay"] >= HALT_MIN_CS,
           "konsole.gif haelt am Schluss %.1f s an" % (b["frames"][-1]["delay"] / 100.0))
    staende_b = gif_staende(b)
    pruefe(all(wert > 0 for wert in staende_b),
           "kein Einzelbild von konsole.gif ist leer")
    pruefe(all(b_ >= a_ - 1e-9 for a_, b_ in zip(staende_b, staende_b[1:])),
           "konsole.gif loescht nie etwas - der Text waechst nur")
    pruefe(staende_b[-1] > staende_b[0],
           "konsole.gif endet mit mehr Text als sie beginnt (%.2f%% -> %.2f%%)"
           % (staende_b[0] * 100, staende_b[-1] * 100))

    # --- Mitschnitt und Rueckgabewerte ------------------------------------
    mit = (ordner / "konsole.txt").read_text(encoding="utf-8")
    befehle = [z[2:] for z in mit.splitlines() if z.startswith("$ ")]
    ausgabe = [z[2:] if z.startswith("| ") else "" for z in mit.splitlines()
               if z.startswith("|")]
    pruefe(any(z.startswith("redact-rs kontoauszug.pdf") for z in befehle),
           "konsole.txt zeigt den gewoehnlichen Aufruf")
    pruefe(any("--check-leaks" in z for z in befehle),
           "konsole.txt zeigt die Nachpruefung")
    pruefe(any(z.startswith("nicht gefunden:") for z in
               (a_.strip() for a_ in ausgabe)),
           "konsole.txt haelt fest, dass der Suchbegriff weg ist")
    pruefe(befehle[-1] == "echo $?" and ausgabe[-1].strip() == "0",
           "konsole.txt endet mit dem echten Rueckgabewert 0")

    pr = (ordner / "pruefung.txt").read_text(encoding="utf-8")
    pruefe(pr.count("Rueckgabewert: 3") >= 2,
           "pruefung.txt haelt beide Male den Rueckgabewert 3 fest")
    pruefe("nicht gefunden: DE89 3704 0044 0532 0130 00" in pr,
           "pruefung.txt zeigt die verschwundene IBAN")
    pruefe("GEFUNDEN" in pr.split("NACHHER")[-1],
           "pruefung.txt verschweigt nicht, was nach dem Lauf noch dasteht")

    return ende()


def ende():
    for zeile in hinweise:
        print(zeile)
    for zeile in fehler:
        print(zeile)
    if fehler:
        print("\n%d Pruefung(en) fehlgeschlagen." % len(fehler))
        return 1
    print("\nAlle %d Pruefungen bestanden." % len(hinweise))
    return 0


if __name__ == "__main__":
    sys.exit(main())
