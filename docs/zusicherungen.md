# Sicherheit — was zugesichert wird

Diese Seite nennt nur die Zusicherungen. Bedrohungsmodell, Grenzen für
Eingabedateien und die zugehörigen Messungen stehen in
[`SECURITY.md`](../SECURITY.md); was das Werkzeug fachlich *nicht* kann, steht
unter [Was dieses Werkzeug nicht leistet](grenzen.md#grenzen).

**Was zugesichert wird**

* Text, den die Analyse gefunden hat, wird aus dem Content-Stream **entfernt**,
  nicht übermalt. Für die geprüften Fälle ist das mit
  [`redact_pdf::leaks`](pruefen.md#pruefen) an der geschriebenen Datei abgesichert — also
  mit einem Orakel, das nicht auf dem eigenen Extraktor beruht.
* **Pixel unter einer Schwärzung werden überschrieben**, nicht überdeckt. Das
  gilt für Bild-XObjects wie für Inline-Bilder, auch in Form-XObjects und auch
  bei gedrehten oder skalierten Platzierungen; wird dasselbe Bild von mehreren
  Seiten benutzt, bekommt die geschwärzte Seite eine eigene Kopie. Lässt sich
  ein betroffenes Bild nicht dekodieren, **bricht der Lauf ab**, statt eine
  Datei zu erzeugen, deren Schwärzung nur obenauf liegt ([Details](bilder.md#bilder)).
* Text in Form-XObjects wird ebenfalls entfernt. Wird dasselbe XObject mehrfach
  platziert, wirkt die Entfernung auf alle Platzierungen — im Zweifel wird also
  eher zu viel als zu wenig geschwärzt.
* Annotationen, die in einen Schwärzungsbereich ragen, werden gelöscht.
* **Metadaten und Restdaten werden entfernt** — der Content-Stream ist nur
  *eine* der Stellen, an denen ein Geheimnis in einer PDF-Datei steht:

  | Woher | Was |
  |---|---|
  | Trailer | `/Info` (Titel, Autor …) |
  | Trailer | `/ID` bekommt eine **neue** Kennung (MD5 der geschriebenen Datei, beide Hälften gleich) — die alte ist eine beliebige Zeichenkette |
  | jedes Objekt | Zeichenketten mit festem Wertebereich bleiben nur **im** Wertebereich: `/Lang` als Sprachangabe (`de`, `de-DE`, `zh-Hans-CN`, `es-419`); `/CIDSystemInfo` mit `/Registry` `Adobe` und einer bekannten `/Ordering`, sonst ersetzt (beide sind Pflicht); `/FontFamily` am `/FontDescriptor` fällt |
  | Katalog | XMP (`/Metadata`), `/PieceInfo`, `/StructTreeRoot` samt `/MarkInfo`, `/Names` und `/Dests` |
  | Katalog | `/AcroForm` — **samt `/XFA`** (ein vollständiger zweiter Formulardatensatz als XML) |
  | jedes Formularfeld | die Werte `/V`, `/DV` und `/RV` — auch bei Widgets, die nur noch über `/Annots` erreichbar sind |
  | Katalog | `/OpenAction` und `/AA` — Aktionen, die beim Öffnen bzw. bei Ereignissen laufen und `/S /JavaScript` sein dürfen |
  | Katalog | `/OCProperties` — die Verwaltung optionaler Inhalte („Ebenen“) |
  | Katalog | `/Outlines` — die Lesezeichen; jeder `/Title` ist frei wählbarer Text („Kontoauszug DE89 …“) |
  | Katalog, jeder Knoten des Seitenbaums, jede Seite | **jeder Schlüssel, der nicht auf der Erlaubnisliste steht** — am Katalog bleiben nur `/Type`, `/Version`, `/Extensions`, `/Pages`, `/PageLabels`, `/OutputIntents`, `/ViewerPreferences`, `/PageLayout`, `/PageMode`, `/Lang`, `/NeedsRendering`, `/AF`; an der Seite Aufbau, Seitenrahmen, Inhalt und Darstellung. Was fällt, auch ohne dass es jemand beim Namen kennt: `/SpiderInfo` (Web Capture: abgerufene Adresse, gesendete Formulardaten), `/Legal`, `/Requirements`. `/PageLabels` und `/OutputIntents` (auch an der Seite) verlieren ihre Texte; und in **jedem Ressourcenverzeichnis** (`/Resources` an Seite, Seitenbaum, Formular, Erscheinungsbild) jeder Schlüssel, der keine der acht Ressourcenarten ist — die Eigenschaftslisten unter `/Properties` bleiben |
  | jedes Objekt **unter** einem erlaubten Schlüssel | jeder Schlüssel, den die Norm an seiner Art nicht kennt: unter `/ViewerPreferences`, `/Extensions` (dort fällt auch `/URL`), in einem `/PageLabels`-Eintrag, unter `/Group`, `/Trans` und `/BoxColorInfo`; an Schrift, Schriftbeschreibung (dort fallen auch `/CharSet` und `/Style`), Kodierung, Grafikzustand, Softmaske, Funktion, Shading, Muster, Bild- und Formular-XObject (dort fallen auch `/ID`, `/Measure`, `/PtData`), `/OCG` und `/OCMD`. Was zeichnet, steht in der Norm und bleibt. Eine Type3-Schrift, die kein Inhaltsstrom setzt, verliert ihre Glyphprozeduren (`/CharProcs`) |
  | jede Seite | `/Metadata`, `/PieceInfo`, `/StructParents`, `/AA` |
  | jede Seite | Annotationen vom Typ `/FileAttachment` — ein Dateianhang klebt nicht nur im `/Names`-Baum |
  | jede verbliebene Annotation — auch eine, die keine Seite in `/Annots` führt, sondern irgendein anderer Schlüssel hält — **und alles, was sie erreichbar hält** (`/Popup`, `/Parent`-Kette, `/Kids`, `/IRT`) | die Aktionen `/A`, `/AA`, `/PA` und ein benanntes `/Dest` (ein ausdrückliches Ziel bleibt, auch hinter einem Verweis — ein Verweis im Feld muss auf eine Seite führen); die Klartexte `/Contents`, `/RC`, `/T`, `/Subj`, `/TU`, `/TM`, `/Opt`, `/OverlayText`, `/NM`, `/DS` und die `/MK`-Beschriftungen; `/M`, `/CreationDate`, `/State`, `/StateModel` und `/Lang`, wenn sie kein Datum, kein Zustand der Norm oder keine Sprachangabe sind; dazu die Beiwerk-Dictionaries `/Movie`, `/Measure`, `/RichMediaContent`, `/RichMediaSettings`, die externen Daten `/ExData` und die 3D-Einheiten `/3DU` als Ganzes (Filmdateiname, Maßangaben, eingebettete Medien-Dateien auch unter den Einstellungen; Preis: ein Film und eine Vermessung sind danach nur noch Bild) und `/Alt`/`/ActualText` an jedem erreichten Dictionary; dahinter jeder Schlüssel, den die Norm am Untertyp der Annotation nicht kennt. **Nicht** `/DA` (benannte Lücke, siehe `SECURITY.md`) — was eine Annotation *zeichnet* (`/AP`), geht wie Seitentext durch die Schwärzung |

  Preis: benannte Sprünge, Lesezeichen und Verweise ins Netz funktionieren
  danach nicht mehr, und aus einem Formular wird ein totes Blatt Papier. Das ist die sichere Richtung.
  Objekte, die dadurch unerreichbar werden, werden zusätzlich aus der Datei
  geworfen (`prune_unreachable`) — `lopdf` schriebe sonst auch alles mit, was
  niemand mehr referenziert.

  So sieht das an einer Datei aus, die all das trägt:

  ```console
  $ redact-rs meta.pdf -o meta_out.pdf --booking-list buchungen.csv
  Metadaten entfernt: /Info-Dictionary, Formulardefinition (/AcroForm),
  XFA-Formulardaten (/XFA), Öffnen-Aktion (/OpenAction), Ebenen (/OCProperties),
  2 Feldwerte, 1 Dateianhang-Annotation, 2 Ereignisaktionen (/AA)
  ```

  Mit [`redact_pdf::leaks`](pruefen.md#pruefen) an der Ausgabedatei nachgeprüft: Feldwert,
  XFA-Inhalt, `/Info`-Titel, OpenAction-JavaScript, Dateianhang und der Name
  einer Ebene (`/OCG /Name`) sind restlos weg. Der Ebenenname war zuletzt die
  einzige verbliebene Fundstelle; er wird jetzt auch dann geleert, wenn eine
  Seite das `/OCG` über `/Resources /Properties` weiterhin referenziert und es
  deshalb das Entfernen von `/OCProperties` überlebt.
* Alles läuft lokal und im Speicher; es werden keine Netzverbindungen
  aufgebaut. Beim Schreiben entsteht genau **eine** temporäre Datei, und zwar
  im Zielverzeichnis (`.<name>.redact-<pid>-<n>.tmp`); sie wird per `rename`
  zur Ausgabedatei, damit nie eine halb geschriebene Datei sichtbar wird. In
  `/tmp` landet nichts.
* Gleiche Eingabe + gleiche Konfiguration ⇒ byteweise gleiche Ausgabe.
* Verschlüsselte PDFs werden ohne Passwort **abgelehnt**; strukturell defekte
  werden abgelehnt, nicht repariert. Das Passwort steht in keiner erzeugten
  Datei und in keiner Meldung
  ([`SECURITY.md`](../SECURITY.md#passwörter-verschlüsselter-pdfs)).
* **Kein Kernabzug: unter Linux.** Stürzt der Prozess ab, schriebe der Kernel
  sonst den ganzen Arbeitsspeicher weg — samt Klartext des Dokuments und
  eingegebenem Passwort. `redact-rs` schaltet das als erste Anweisung in `main`
  ab (`prctl(PR_SET_DUMPABLE, 0)`). **Unter macOS, BSD und den übrigen
  Unix-Systemen setzt es stattdessen `setrlimit(RLIMIT_CORE, 0)` — eine Grenze,
  kein Verbot. Unter Windows ist nichts umgesetzt**; die Einschränkung gehört
  zur Zusage dazu und steht ausgeschrieben in
  [`SECURITY.md`](../SECURITY.md#kein-kernabzug-dieses-prozesses). Dort steht auch
  der Nebeneffekt: der Prozess ist danach **unter Linux** für `ptrace` durch
  denselben Benutzer unerreichbar — auf den übrigen Unix-Systemen hat
  `setrlimit(RLIMIT_CORE, 0)` diesen Nebeneffekt nicht.
* Review-Datei und Audit-Log entstehen unter Unix mit Modus `0600` — **in
  beiden Programmen**. Sie gehen durch denselben Schreibpfad
  (`redact_pipeline::write_review_file` bzw. `AuditLog::write`, beide über
  `redact_pdf::document::write_file` mit `secret_options`), und beide Programme
  rufen ihn. Gemessen an der geschriebenen Datei: die Oberfläche in
  `export_removes_the_text_from_the_pdf`
  (`crates/redact-gui/src/state.rs`, `assert_eq!(mode, 0o600)` auf das
  Audit-Log), beide Wege nebeneinander in `both_ways_write_the_same_review_file`
  (`crates/redact-cli/tests/cli_and_gui_agree.rs`). Mit `0644` und ohne
  Symlink-Prüfung entstanden sie in der Oberfläche früher — das ist geschlossen.
