# Probenliste

Der Gegenstand, an dem die Fix-Runden enden — nicht „nichts gefunden“, das ist
eine Abwesenheitsbehauptung, sondern: **jede Zeile dieser Liste ist in zwei
Runden hintereinander grün und mutationsfest.** Das ist eine Checkliste, und
eine Checkliste hat ein Ende. Wie die Runden laufen, steht in
[`CONTRIBUTING.md`](CONTRIBUTING.md), „Drei Spuren, eine Probenliste, ein
Ende“.

Jede Zeile ist eine Fehlerklasse, die in dieser Schleife mindestens einmal
aufgetreten ist, mit der Runde ihres ersten Auftretens und den Belegdateien,
die sie festhalten. Eine neue Klasse kommt als **neue Zeile** dazu, nicht in
eine bestehende gequetscht — dann steht sichtbar, dass der Umfang gewachsen
ist, und die Runde, die sie fand, ist keine, die „nicht konvergiert“.

`crates/redact-cli/tests/zn_c_probenliste.rs` prüft, dass jede genannte
Belegdatei existiert. Ohne das wäre die Liste nach dem nächsten Umbenennen
Prosa.

| Klasse | Spur | erstmals | Beleg |
|---|---|---|---|
| Spiegel über Formular: verschachtelt, mehrfach platziert, ohne eigenes `/Resources`, in `/Properties` | A | R3, R6, R7 | `crates/redact-pdf/tests/zg_r1_spiegel.rs`, `crates/redact-pdf/tests/zg_r1_decke.rs` |
| Bild unter zwei Namen; Kopie, Teilung, Waise | A | R8, R9 | `crates/redact-pdf/tests/zm_a_zwei_namen_ein_bild.rs`, `crates/redact-pdf/tests/zm_a_kopie_teilung_und_waise.rs` |
| Metadaten-Träger, die den Lauf überstehen (`/Popup`, `/AF`, `/PieceInfo`, `/Movie`, `/RichMediaContent`, …) | A | R5, R7 | `crates/redact-pdf/tests/zg_r3_beiwerk.rs`, `crates/redact-pdf/tests/zg_r3_barrierefrei.rs` |
| Filterkette: unbekannter Filter an erster Stelle, `/Filter`-Wert kein Name, Kette weggeworfen | A | R4, R5, R7 | `crates/redact-pdf/tests/ze_p1_budget_und_filter.rs`, `crates/redact-cli/tests/zf_q5_unbekannter_filter.rs`, `crates/redact-pdf/tests/zg_r2_unbrauchbarer_filterwert.rs` |
| Verschachtelungstiefe: stille Entwarnung jenseits der Decke | A | R5 | `crates/redact-cli/tests/ze_p4_check_leaks_grenzen.rs` |
| Flächenfrage am Rand: berührt ist nicht geschnitten; entartete Matrix | A | R9 | `crates/redact-pdf/tests/zm_d_bildwahrheit_gegengelesen.rs`, `crates/redact-pdf/tests/zm_a_entartete_ctm.rs` |
| Kollisionsschutz auf einem Dateisystem ohne Groß-/Kleinschreibung | A | R9 (CI) | `crates/redact-gui/tests/zm_b_kennung_der_ausgabedatei.rs`, `crates/redact-gui/src/zn_b_schreibweise_tests.rs` |
| Ort einer Fundstelle nur als Text — die Oberfläche kann „stehen gelassen“ nicht von „danebengegangen“ unterscheiden | A | R7 | `crates/redact-pdf/tests/zh_a_ort_maschinenlesbar.rs` |
| Plattformfremder Test: eine Einrichtung des Systems beim Namen genannt, ohne `cfg` und ohne Ausweg | B | #13, #33, #43 | `crates/redact-cli/tests/zf_q5_plattformzusagen.rs` |
| Scheintest, ungebundene Zahl, Kopie eines Helfers | B | R3, R5, R6 | `crates/redact-cli/tests/belege.rs`; Mutationsnachweis je Korrektur, mit Skript |
| Das Gate passt nicht auf die Platte | B | nach R9 | `crates/redact-gui/tests/zn_a_debug_info_stufe.rs` |
| Seite außerhalb des Seitenbaums, gehalten von `/Dest` oder `/P` | A | Spur-A-Runde 1 (#69) | `crates/redact-pdf/tests/zo_b_traeger.rs` |
| Bild ohne Zeichner: `/Thumb` der Seite | A | Spur-A-Runde 1 (#74) | `crates/redact-pdf/tests/zo_a_bild_ohne_zeichner.rs` |
| Decke umgangen durch die Zahlenart (`/Width` als reelle Zahl) | A | Spur-A-Runde 1 (#79) | `crates/redact-pdf/tests/zo_a_maske_und_filter.rs` |
| Spiegel über Formular: zwei Umgebungen, beide mit eigenem Spiegel | A | Spur-A-Runde 1 (#65) | `crates/redact-pdf/tests/zo_c_spiegel_umgebungen.rs` |
| Spiegel im Formular mit eigenen Ressourcen ohne `/Properties`, Name nur beim Aufrufer auflösbar (Poppler) | A | Spur-A-Runde 1 (#67), Runde 2 (#86: Eintrag `null`; #85: Annotationserscheinung) | `crates/redact-pdf/tests/zo_c_spiegel_umgebungen.rs` |
| Stencil-`/Mask` eines geschwärzten Bildes trägt die Form unter der Zone | A | Spur-A-Runde 1 (#77) | `crates/redact-pdf/tests/zo_a_maske_und_filter.rs` |
| Beiwerk mit Klartext an Katalog, Seite, Objekt und Signaturfeld (`/Perms`, `/DSS`, `/PageLabels`, `/Threads`, `/Collection`, `/OutputIntents`, `/URI`, `/DPartRoot`, `/VP`, `/PresSteps`, `/Ref`, `/OPI`, `/SV`, `/Lock`) | A | Spur-A-Runde 1 (#70) | `crates/redact-pdf/tests/zo_b_traeger.rs` |
| Verweiskette: eine Karte, die je Glied die ganze Kette läuft (quadratisch) | A | Spur-A-Runde 1 (#72) | `crates/redact-pdf/tests/zo_b_traeger.rs` |
| Bild ohne Zeichner: das Original nach der Kopie, erreichbar hinter geerbtem oder überzähligem Namen | A | Spur-A-Runde 1 (#76) | `crates/redact-pdf/tests/zo_a_bild_ohne_zeichner.rs` |
| Bild ohne Zeichner: Bild oder Formular im Kachelmuster ohne Text (Ausstieg allein am Textoperator) | A | Spur-A-Runde 1 (#75) | `crates/redact-pdf/tests/zo_a_bild_ohne_zeichner.rs` |
| Spiegel über Kachelmuster: der Geltungsbereich kennt nur `Do`, nicht `scn` | A | Spur-A-Runde 1 (#66) | `crates/redact-pdf/tests/zo_c_spiegel_umgebungen.rs` |
| Erscheinungsstrom, den niemand liest: `/MK /I`, `/RI`, `/IX`; `/AP` einer nur über `/Popup`, `/Parent`, `/IRT` gehaltenen Annotation | A | Spur-A-Runde 1 (#71) | `crates/redact-pdf/tests/zo_b_traeger.rs` |
| Gehaltene Spiegel in Formularen über alle Seiten, ohne Decke | A | Spur-A-Runde 1 (#68) | `crates/redact-pdf/tests/zo_c_spiegel_umgebungen.rs` |
| Abgelehnte gewöhnliche Datei: ein Bildfilter, den das Orakel kann und der Bilddekoder nicht (`LZWDecode`) | A | Spur-A-Runde 1 (#78) | `crates/redact-pdf/tests/zo_a_maske_und_filter.rs` |
| Vorprüfung: eine feste Grenze der Rohgröße lehnt ein großes ASCII85-Bild ab, obwohl der Dekoder begrenzt ist | A | Spur-A-Runde 1 (#82) | `crates/redact-cli/tests/zo_d_orakel_am_binary.rs` |
| Filterkette: ein Glied oder eine Schreibweise, die die Vorprüfung nicht auspackt, nimmt der Kette die Entpackgrenze | A | Spur-A-Runde 1 (#64, Nachtrag #83: der Vorspann vor einem Bildfilter), Runde 2 (#89: Verweis in der Kette, rohes Deflate, Teilergebnis, `/Filter` anders geschrieben; #99: gebucht die Ausgabe des letzten Glieds statt der Arbeit der Kette) | `crates/redact-cli/tests/zo_e_kettenbombe_schreibpfad.rs`, `crates/redact-cli/tests/zo_e_vorspann_vor_bildfilter.rs`, `crates/redact-cli/tests/zp_e_vorpruefung_als_schranke.rs`, `crates/redact-pdf/tests/zp_e_teilergebnis_zaehlt.rs`, `crates/redact-pdf/tests/zp_d_arbeit_der_kette.rs` |
| Orakel: eine Zeichenkettenkodierung der Norm, die der Dekoder nicht kennt (PDFDocEncoding 0x80–0xA0) | A | Spur-A-Runde 1 (#81) | `crates/redact-pdf/tests/zo_d_altgeneration_und_kodierung.rs`, `crates/redact-cli/tests/zo_d_orakel_am_binary.rs` |
| Orakel: eine Altgeneration unter einem Filter, den nur die Objektsicht kann, oder als maskierte Zeichenkette | A | Spur-A-Runde 1 (#80) | `crates/redact-pdf/tests/zo_d_altgeneration_und_kodierung.rs`, `crates/redact-cli/tests/zo_d_orakel_am_binary.rs` |
| Bilddecke: der Dekoder belegt andere Maße als die, gegen die die Decke geprüft hat (JPEG-Kopf gegen Dictionary) | A | Spur-A-Runde 2 (#101) | `crates/redact-cli/tests/zp_a_jpeg_groesser_als_angegeben.rs` |
| Abgelehnte gewöhnliche Datei: Leerraum der Norm, den der Zerleger nicht kennt (Kommentar vor einer Leerzeile, NUL, Seitenvorschub) | A | Spur-A-Runde 2 (#103) | `crates/redact-pdf/tests/zp_d_kommentar_und_leerraum.rs` |
| Orakel: eine Seite, die der Interpreter ablehnt, oder ein Strom, den der Lader anders las als die Rohbytes, fehlt ohne Meldung in einer Sicht | A | Spur-A-Runde 2 (#98) | `crates/redact-pdf/tests/zp_d_seite_und_lader.rs` |
| Ressourcenname nur beim Aufrufer auflösbar: `/Font`, `/XObject`, `/ExtGState`, `/Pattern` aus einem Formular mit eigenem `/Resources` (Poppler zeichnet) | A | Spur-A-Runde 2 (#88) | `crates/redact-pdf/tests/zp_c_ressourcen_beim_aufrufer.rs` |

Was nur die CI sehen kann, steht dabei: die Zeile zur Groß-/Kleinschreibung
prüft ihren positiven Zweig allein im Job „Build (windows-2025)“.
