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
| Stencil-`/Mask` eines geschwärzten Bildes trägt die Form unter der Zone | A | Spur-A-Runde 1 (#77) | `crates/redact-pdf/tests/zo_a_maske_und_filter.rs` |
| Beiwerk mit Klartext an Katalog, Seite, Objekt und Signaturfeld (`/Perms`, `/DSS`, `/PageLabels`, `/Threads`, `/Collection`, `/OutputIntents`, `/URI`, `/DPartRoot`, `/VP`, `/PresSteps`, `/Ref`, `/OPI`, `/SV`, `/Lock`) | A | Spur-A-Runde 1 (#70) | `crates/redact-pdf/tests/zo_b_traeger.rs` |

Was nur die CI sehen kann, steht dabei: die Zeile zur Groß-/Kleinschreibung
prüft ihren positiven Zweig allein im Job „Build (windows-2025)“.
