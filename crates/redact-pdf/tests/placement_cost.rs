//! Was eine **Platzierung** kosten darf.
//!
//! Ein Form-XObject wird mit sechs Byte gezeichnet (`q Do Q`). Was hinter
//! diesen sechs Byte an Arbeit hängt, entscheidet, ob eine kleine Datei den
//! Rechner beschäftigen kann:
//!
//! * Vorher wurde der Strom des Formulars bei **jeder** Platzierung erneut
//!   ausgepackt und zerlegt, und sein `/Resources`-Verzeichnis erneut in
//!   Schriftmetriken übersetzt. Beides fällt **vor** der ersten gezählten
//!   Zeichenoperation an — das Aufwandskonto sah es also nicht. Gemessen an
//!   einem Formular mit 100 Schriften (je 500 CMap-Einträge): 400
//!   Platzierungen 31,9 s, 800 Platzierungen 64,0 s, 1 600 Platzierungen
//!   126,5 s, bei einer Datei, die dabei um 27 Byte wuchs.
//! * Nachher wird jedes Stromobjekt genau einmal ausgepackt und jedes
//!   Ressourcenverzeichnis genau einmal geladen.
//!
//! ## Warum hier keine Uhr steht
//!
//! Eine Zeitmessung im Testlauf flattert mit der Fremdlast der Maschine.
//! Gemessen wird deshalb, was an der Datei hängt und nicht am Rechner:
//! [`redact_pdf::content::ScanEffort`] — wie oft ein Strom ausgepackt und wie
//! oft ein Ressourcenverzeichnis geladen wurde. Diese Zahlen sind
//! deterministisch; sie dürfen mit der Zahl der Platzierungen **nicht**
//! wachsen.
//!
//! ## Und was der Zwischenspeicher nicht darf
//!
//! Ein gemerktes Formular ist nicht in jedem Zusammenhang dasselbe: dieselbe
//! Objekt-Id, aber möglicherweise andere **geerbte** Ressourcen. Genau das
//! prüft [`ein_geerbtes_verzeichnis_bleibt_das_des_aufrufers`] — ein
//! Zwischenspeicher, der auch das mitmerkte, wäre ein stiller Fehler und
//! damit schlimmer als die langsame Schleife, die er ersetzt.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, scan_page, PdfRedactor, ShowItem};

/// Das Geheimnis, das in den Schwärzungsfällen steht.
const SECRET: &str = "DE89 3704 0044 0532 0130 00";

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

/// Ein Grundgerüst mit genau einer Seite.
struct Doc {
    doc: Document,
    page_id: ObjectId,
    content_id: ObjectId,
    resources_id: ObjectId,
}

impl Doc {
    fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let resources_id = doc.add_object(dictionary! {});
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1_i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        Self {
            doc,
            page_id,
            content_id,
            resources_id,
        }
    }

    fn add(&mut self, object: impl Into<Object>) -> ObjectId {
        self.doc.add_object(object)
    }

    fn set_content(&mut self, raw: impl AsRef<[u8]>) {
        self.doc.objects.insert(
            self.content_id,
            Object::Stream(Stream::new(dictionary! {}, raw.as_ref().to_vec())),
        );
    }

    fn set_resources(&mut self, dict: Dictionary) {
        self.doc
            .objects
            .insert(self.resources_id, Object::Dictionary(dict));
    }

    fn bytes(&self) -> Vec<u8> {
        save_to_bytes(&self.doc).expect("speicherbar")
    }
}

/// Eine Helvetica ohne `/ToUnicode`.
fn helvetica(doc: &mut Doc) -> ObjectId {
    doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    })
}

/// Eine Helvetica mit einer `/ToUnicode`-CMap, die `codes` auf `texts`
/// abbildet — damit lässt sich am **dekodierten Text** ablesen, welche
/// Schrift der Interpreter benutzt hat.
fn font_with_cmap(doc: &mut Doc, pairs: &[(u8, char)], filler: usize) -> ObjectId {
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Test def /CMapType 2 def\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n",
    );
    for (code, text) in pairs {
        cmap.push_str(&format!(
            "1 beginbfchar <{:02X}> <{:04X}> endbfchar\n",
            code, *text as u32
        ));
    }
    // Ballast, damit das Parsen der CMap messbar Arbeit ist.
    for extra in 0..filler {
        let code = 0x80 + (extra % 0x40);
        cmap.push_str(&format!(
            "1 beginbfchar <{:02X}> <{:04X}> endbfchar\n",
            code,
            0x100 + extra
        ));
    }
    cmap.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");
    let cmap_id = doc.add(Stream::new(dictionary! {}, cmap.into_bytes()));
    doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
        "ToUnicode" => cmap_id,
    })
}

/// Ein Form-XObject mit eigenem `/Resources`.
fn form_with_resources(doc: &mut Doc, resources: ObjectId, content: &str) -> ObjectId {
    doc.add(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => resources,
        },
        content.as_bytes().to_vec(),
    ))
}

/// Ein Form-XObject **ohne** eigenes `/Resources` — es erbt die des Aufrufers.
fn form_inheriting(doc: &mut Doc, content: &str) -> ObjectId {
    doc.add(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        },
        content.as_bytes().to_vec(),
    ))
}

/// Eine Schwärzung von Hand über `rect`.
fn blackout(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Test".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Der gesetzte Text aller Datensätze eines Scans, je Datensatz eine
/// Zeichenkette.
fn shown_texts(scan: &redact_pdf::ScanResult) -> Vec<String> {
    scan.shows
        .iter()
        .map(|record| {
            record
                .items
                .iter()
                .filter_map(|item| match item {
                    ShowItem::Glyph(g) => Some(g.text.as_str()),
                    ShowItem::Adjust(_) => None,
                })
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Befund 1 — der Preis einer Platzierung
// ---------------------------------------------------------------------------

/// Baut eine Seite, die dasselbe Formular `placements`-mal zeichnet.
///
/// Das Formular bringt `fonts` Schriften mit; jede trägt eine CMap, deren
/// Parsen Arbeit macht.
fn page_with_repeated_form(placements: usize, fonts: usize) -> Doc {
    let mut doc = Doc::new();
    let mut font_dict = Dictionary::new();
    for index in 0..fonts {
        let font = font_with_cmap(&mut doc, &[], 200);
        font_dict.set(format!("F{index}"), font);
    }
    let form_resources = doc.add(dictionary! { "Font" => font_dict });
    let form = form_with_resources(
        &mut doc,
        form_resources,
        &format!("BT /F0 12 Tf 20 20 Td ({SECRET}) Tj ET\n"),
    );
    doc.set_resources(dictionary! { "XObject" => dictionary! { "Fx" => form } });
    doc.set_content("q /Fx Do Q\n".repeat(placements));
    doc
}

/// Ein hundertmal gezeichnetes Formular wird **einmal** ausgepackt.
#[test]
fn ein_mehrfach_platziertes_formular_wird_einmal_ausgepackt() {
    let doc = page_with_repeated_form(100, 5);
    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");

    assert_eq!(
        scan.shows.len(),
        100,
        "jede Platzierung muss ihre eigene Geometrie liefern"
    );
    assert_eq!(scan.form_placements.values().sum::<usize>(), 100);
    assert_eq!(
        scan.effort.decoded_streams, 1,
        "der Formularstrom darf nur einmal ausgepackt werden, nicht je Platzierung"
    );
    assert_eq!(
        scan.effort.loaded_font_maps, 2,
        "genau zwei Verzeichnisse: das der Seite und das des Formulars"
    );
}

/// Die Vorarbeit hängt an der Datei, nicht an der Zahl der Platzierungen.
///
/// Das ist die eigentliche Schranke: von 10 auf 1 000 Platzierungen wächst
/// die Datei um knapp 10 kByte — vorher wuchs die Vorarbeit um den Faktor
/// 100 mit.
#[test]
fn der_aufwand_haengt_nicht_an_der_zahl_der_platzierungen() {
    let wenige = page_with_repeated_form(10, 8);
    let viele = page_with_repeated_form(1_000, 8);

    let a = scan_page(&wenige.doc, wenige.page_id)
        .expect("lesbar")
        .effort;
    let b = scan_page(&viele.doc, viele.page_id).expect("lesbar").effort;

    assert_eq!(a, b, "hundertfache Platzierung, gleiche Vorarbeit");
    assert_eq!(a.decoded_streams, 1);
    assert_eq!(a.loaded_font_maps, 2);
}

/// Auch die **Schachtelung** zahlt je Objekt, nicht je Weg dorthin.
///
/// Zwei Ebenen mit je 20 Platzierungen ergeben 400 Durchläufe durch das
/// innere Formular — ausgepackt wird trotzdem zweimal, einmal je Objekt.
#[test]
fn geschachtelte_formulare_zahlen_je_objekt() {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    let inner_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let inner = form_with_resources(
        &mut doc,
        inner_resources,
        "BT /F1 6 Tf 1 1 Td (innen) Tj ET\n",
    );
    let outer_resources = doc.add(dictionary! {
        "Font" => dictionary! { "F1" => font },
        "XObject" => dictionary! { "In" => inner },
    });
    let outer = form_with_resources(&mut doc, outer_resources, &"q /In Do Q\n".repeat(20));
    doc.set_resources(dictionary! { "XObject" => dictionary! { "Out" => outer } });
    doc.set_content("q /Out Do Q\n".repeat(20));

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    assert_eq!(scan.shows.len(), 400, "20 × 20 Durchläufe");
    assert_eq!(scan.effort.decoded_streams, 2, "zwei Formularobjekte");
    assert_eq!(
        scan.effort.loaded_font_maps, 3,
        "Seite, äußeres und inneres Verzeichnis"
    );
}

/// **Der Kern des Zwischenspeichers:** ein Formular ohne eigenes
/// `/Resources` erbt die des Aufrufers — und die sind je Platzierung andere.
///
/// Dasselbe Formularobjekt wird aus zwei Umgebungen gezeichnet, die unter
/// demselben Ressourcennamen `/F1` **verschiedene** Schriften anbieten. Die
/// beiden `/ToUnicode`-CMaps bilden dieselben Codes auf verschiedene Zeichen
/// ab; am dekodierten Text lässt sich deshalb ablesen, welche Schrift der
/// Interpreter benutzt hat.
///
/// Ein Zwischenspeicher, der die **aufgelösten** Ressourcen mitmerkte, gäbe
/// hier zweimal dieselbe Antwort — und die Schwärzung suchte im zweiten
/// Zusammenhang nach dem falschen Text. Kein Fehler, den irgendetwas meldete:
/// genau der stille.
#[test]
fn ein_geerbtes_verzeichnis_bleibt_das_des_aufrufers() {
    let mut doc = Doc::new();
    let font_a = font_with_cmap(&mut doc, &[(b'A', 'X'), (b'B', 'Y')], 0);
    let font_b = font_with_cmap(&mut doc, &[(b'A', 'P'), (b'B', 'Q')], 0);

    let shared = form_inheriting(&mut doc, "BT /F1 12 Tf 20 20 Td (AB) Tj ET\n");

    let resources_a = doc.add(dictionary! {
        "Font" => dictionary! { "F1" => font_a },
        "XObject" => dictionary! { "S" => shared },
    });
    let resources_b = doc.add(dictionary! {
        "Font" => dictionary! { "F1" => font_b },
        "XObject" => dictionary! { "S" => shared },
    });
    let wrapper_a = form_with_resources(&mut doc, resources_a, "q 1 0 0 1 0 700 cm /S Do Q\n");
    let wrapper_b = form_with_resources(&mut doc, resources_b, "q 1 0 0 1 0 600 cm /S Do Q\n");

    doc.set_resources(dictionary! {
        "XObject" => dictionary! { "A" => wrapper_a, "B" => wrapper_b },
    });
    doc.set_content("q /A Do Q q /B Do Q\n");

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let texts = shown_texts(&scan);
    assert_eq!(
        texts,
        vec!["XY".to_string(), "PQ".to_string()],
        "derselbe Strom, zwei geerbte Schriftverzeichnisse — zwei Ergebnisse"
    );

    // Und trotzdem: jedes Objekt einmal.
    assert_eq!(
        scan.effort.decoded_streams, 3,
        "das geteilte Formular und die zwei Umgebungen"
    );
    assert_eq!(
        scan.effort.loaded_font_maps, 3,
        "Seite und die beiden Umgebungen; das geerbende Formular lädt nichts"
    );
}

/// Dieselbe Zusicherung für die Geometrie: die CTM kommt je Platzierung frisch
/// dazu, sie steckt nicht im gemerkten Strom.
#[test]
fn jede_platzierung_behaelt_ihre_eigene_lage() {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    let form_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let form = form_with_resources(&mut doc, form_resources, "BT /F1 12 Tf 0 0 Td (M) Tj ET\n");
    doc.set_resources(dictionary! { "XObject" => dictionary! { "Fx" => form } });
    doc.set_content(
        "q 1 0 0 1 10 700 cm /Fx Do Q\n\
         q 1 0 0 1 10 500 cm /Fx Do Q\n\
         q 1 0 0 1 10 300 cm /Fx Do Q\n",
    );

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let mut ys: Vec<i64> = scan
        .shows
        .iter()
        .flat_map(|r| r.glyphs())
        .map(|g| g.origin.y.round() as i64)
        .collect();
    ys.sort_unstable();
    assert_eq!(ys, vec![300, 500, 700]);
    assert_eq!(scan.effort.decoded_streams, 1);
}

/// Eine weiche Maske kann an jedem einzelnen `gs` hängen — ausgepackt wird
/// sie einmal.
#[test]
fn eine_weiche_maske_an_vielen_gs_wird_einmal_ausgepackt() {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    let group_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let group = doc.add(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Group" => dictionary! { "S" => "Transparency" },
            "Resources" => group_resources,
        },
        format!("BT /F1 12 Tf 20 700 Td ({SECRET}) Tj ET\n").into_bytes(),
    ));
    let gstate = doc.add(dictionary! {
        "Type" => "ExtGState",
        "SMask" => dictionary! { "S" => "Luminosity", "G" => group },
    });
    doc.set_resources(dictionary! { "ExtGState" => dictionary! { "GS0" => gstate } });
    doc.set_content("q /GS0 gs 0 0 10 10 re f Q\n".repeat(200));

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    assert_eq!(scan.form_placements.get(&group), Some(&200));
    assert_eq!(
        scan.effort.decoded_streams, 1,
        "zweihundert `gs`, ein Auspacken"
    );
    assert_eq!(scan.effort.loaded_font_maps, 2);
}

/// Ein Kachelmuster kann an jedem einzelnen `scn` hängen — ausgepackt wird es
/// einmal.
#[test]
fn ein_kachelmuster_an_vielen_scn_wird_einmal_ausgepackt() {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    let pattern_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let pattern = doc.add(Stream::new(
        dictionary! {
            "Type" => "Pattern",
            "PatternType" => 1_i64,
            "PaintType" => 1_i64,
            "TilingType" => 1_i64,
            "BBox" => vec![0.into(), 0.into(), 300.into(), 40.into()],
            "XStep" => 300_i64,
            "YStep" => 40_i64,
            "Resources" => pattern_resources,
        },
        format!("BT /F1 10 Tf 2 10 Td ({SECRET}) Tj ET\n").into_bytes(),
    ));
    doc.set_resources(dictionary! { "Pattern" => dictionary! { "P0" => pattern } });
    doc.set_content("q /Pattern cs /P0 scn 0 0 100 100 re f Q\n".repeat(200));

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    assert_eq!(
        scan.effort.decoded_streams, 1,
        "zweihundert `scn`, ein Auspacken"
    );
    assert_eq!(scan.effort.loaded_font_maps, 2);
}

/// Zweihundert Annotationen mit demselben Erscheinungsstrom: ein Auspacken,
/// ein Schriftverzeichnis.
#[test]
fn ein_geteilter_erscheinungsstrom_wird_einmal_ausgepackt() {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    let ap_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let appearance = doc.add(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
            "Resources" => ap_resources,
        },
        format!("BT /F1 10 Tf 2 4 Td ({SECRET}) Tj ET\n").into_bytes(),
    ));
    let annots: Vec<Object> = (0..200)
        .map(|k| {
            let y = 20 + (k % 40) * 20;
            Object::Reference(doc.add(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "F" => 4_i64,
                "Rect" => vec![10.into(), y.into(), 310.into(), (y + 20).into()],
                "AP" => dictionary! { "N" => appearance },
            }))
        })
        .collect();
    doc.doc
        .get_dictionary_mut(doc.page_id)
        .expect("Seite")
        .set("Annots", annots);

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    assert_eq!(
        scan.effort.decoded_streams, 1,
        "zweihundert Annotationen, ein Erscheinungsstrom"
    );
    assert_eq!(
        scan.effort.loaded_font_maps, 2,
        "das Verzeichnis der Seite und das des Erscheinungsstroms"
    );
}

// ---------------------------------------------------------------------------
// Befund 2 — die Schwärzung darf nicht jeden Bereich gegen jedes Zeichen prüfen
// ---------------------------------------------------------------------------

/// Eine Seite mit `lines` Zeilen zu je `per_line` Vorkommen des Geheimnisses.
fn page_with_many_secrets(lines: usize, per_line: usize) -> Doc {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut content = String::from("BT /F1 4 Tf\n");
    for line in 0..lines {
        let y = 830 - line * 4;
        let mut text = String::new();
        for _ in 0..per_line {
            text.push_str(SECRET);
            text.push_str("  ");
        }
        content.push_str(&format!("1 0 0 1 5 {y} Tm ({text}) Tj\n"));
    }
    content.push_str("ET\n");
    doc.set_content(content);
    doc
}

/// Alle Vorkommen des Geheimnisses auf der Seite, als Schwärzungen.
fn redactions_for_secret(doc: &Doc) -> Vec<Redaction> {
    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let mut out = Vec::new();
    for record in &scan.shows {
        let glyphs: Vec<_> = record.glyphs().collect();
        let text: String = glyphs.iter().map(|g| g.text.as_str()).collect();
        let mut from = 0usize;
        while let Some(at) = text[from..].find(SECRET) {
            let start = from + at;
            let end = start + SECRET.len();
            let rect = glyphs[start..end]
                .iter()
                .map(|g| g.rect)
                .reduce(|a, b| a.union(&b))
                .expect("nicht leer");
            out.push(blackout(rect));
            from = end;
        }
    }
    out
}

/// Viele Bereiche auf viel Text: das Ergebnis bleibt vollständig.
///
/// Der Prüfmaßstab ist [`leaks`] an der fertigen Datei, nicht der eigene
/// Extraktor: wovor der Extraktor blind ist, wird nicht geschwärzt und wäre
/// für ihn trotzdem unsichtbar.
#[test]
fn viele_bereiche_entfernen_weiterhin_jedes_zeichen() {
    for (lines, per_line) in [(20usize, 1usize), (20, 8), (200, 8)] {
        let mut doc = page_with_many_secrets(lines, per_line);
        let redactions = redactions_for_secret(&doc);
        assert_eq!(
            redactions.len(),
            lines * per_line,
            "die Vorlage muss so viele Treffer haben, wie sie Vorkommen hat"
        );

        // Ohne Rand: der Bereich ist genau die Hülle des Treffers, also darf
        // er genau dessen Zeichen erwischen und kein Leerzeichen daneben.
        let report = PdfRedactor::with_padding(0.0)
            .apply_with_report(&mut doc.doc, &redactions)
            .expect("schwärzbar");
        assert_eq!(
            report.removed_glyphs,
            SECRET.len() * lines * per_line,
            "jedes Zeichen jedes Vorkommens"
        );
        assert!(
            report.per_redaction.iter().all(|n| *n == SECRET.len()),
            "jede einzelne Region muss ihr Vorkommen ganz getroffen haben: {:?}",
            report.per_redaction
        );

        let bytes = doc.bytes();
        assert!(
            leaks(&bytes, SECRET).is_empty(),
            "{lines}×{per_line}: das Geheimnis steht noch in der Datei"
        );
    }
}

/// Ein Bereich, der die Zeile nur **berührt**, entfernt nichts — und ein
/// Bereich weit daneben ebenfalls nicht.
///
/// Die Vorauswahl nach x darf nichts wegwerfen, was die genaue Prüfung noch
/// getroffen hätte; sie darf aber auch nichts hinzuerfinden.
#[test]
fn bereiche_neben_der_zeile_entfernen_nichts() {
    let mut doc = page_with_many_secrets(1, 1);
    let treffer = redactions_for_secret(&doc);
    let auf_der_zeile = treffer[0].region.rect;

    let daneben = |rect: Rect| blackout(rect);
    let breite = auf_der_zeile.ur.x - auf_der_zeile.ll.x;
    let redactions = vec![
        // Weit links, weit rechts, weit oben, weit unten.
        daneben(Rect::new(0.0, auf_der_zeile.ll.y, 1.0, auf_der_zeile.ur.y)),
        daneben(Rect::new(
            auf_der_zeile.ur.x + 10.0 + breite,
            auf_der_zeile.ll.y,
            auf_der_zeile.ur.x + 20.0 + breite,
            auf_der_zeile.ur.y,
        )),
        daneben(Rect::new(
            auf_der_zeile.ll.x,
            auf_der_zeile.ur.y + 40.0,
            auf_der_zeile.ur.x,
            auf_der_zeile.ur.y + 60.0,
        )),
        daneben(Rect::new(
            auf_der_zeile.ll.x,
            auf_der_zeile.ll.y - 60.0,
            auf_der_zeile.ur.x,
            auf_der_zeile.ll.y - 40.0,
        )),
    ];

    // Ohne Rand, sonst greift die Erweiterung um `padding` in die Zeile.
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc.doc, &redactions)
        .expect("schwärzbar");
    assert_eq!(report.removed_glyphs, 0);
    assert_eq!(report.per_redaction, vec![0, 0, 0, 0]);
    assert!(
        !leaks(&doc.bytes(), SECRET).is_empty(),
        "nichts sollte entfernt worden sein"
    );
}

/// Ein Bereich, der die Zeichenhülle nur an der **Kante** berührt, erwischt
/// ein entartetes Zeichen weiterhin.
///
/// Das ist die scharfe Kante der Vorauswahl. Ein Zeichen ohne Breite (hier:
/// `/Widths [0]`) hat eine Fläche von null; [`Rect::covered_fraction`] fällt
/// dafür auf den **Mittelpunkt** zurück, und `Rect::contains` zählt die Kante
/// mit. Eine Vorauswahl, die die Kante ausschlösse — etwa mit
/// `Rect::intersects`, das Berührung nicht als Überlappung zählt —, ließe
/// genau dieses Zeichen stehen. Still, ohne Warnung, mit „0 entfernte
/// Zeichen“ im Bericht.
#[test]
fn ein_bereich_an_der_kante_erwischt_ein_entartetes_zeichen() {
    let mut doc = Doc::new();
    // Eine Schrift, deren einziges Zeichen die Breite 0 hat.
    let font = doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "AAAAAA+Null",
        "Encoding" => "WinAnsiEncoding",
        "FirstChar" => 65_i64,
        "LastChar" => 65_i64,
        "Widths" => vec![0.into()],
    });
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    doc.set_content("BT /F1 10 Tf 100 700 Td (A) Tj ET\n");

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    let glyph = scan.shows[0].glyphs().next().expect("ein Zeichen").rect;
    assert_eq!(glyph.ll.x, glyph.ur.x, "das Zeichen muss entartet sein");

    // Der Bereich endet exakt an der Senkrechten des Zeichens.
    let rect = Rect::new(glyph.ll.x - 10.0, glyph.ll.y, glyph.ll.x, glyph.ur.y);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc.doc, &[blackout(rect)])
        .expect("schwärzbar");
    assert_eq!(
        report.removed_glyphs, 1,
        "die Kante zählt mit — sonst bleibt das Zeichen still stehen"
    );
    assert_eq!(report.per_redaction, vec![1]);
}

/// Überlappende Bereiche werden weiterhin **jeder für sich** gezählt.
///
/// Die Vorauswahl entscheidet nur, *welche* Bereiche geprüft werden; an der
/// Buchführung je Region darf sie nichts ändern.
#[test]
fn ueberlappende_bereiche_zaehlen_weiterhin_einzeln() {
    let mut doc = page_with_many_secrets(1, 1);
    let treffer = redactions_for_secret(&doc);
    let rect = treffer[0].region.rect;
    // Zweimal derselbe Bereich, einmal ein größerer darüber.
    let redactions = vec![blackout(rect), blackout(rect), blackout(rect.expanded(2.0))];

    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc.doc, &redactions)
        .expect("schwärzbar");
    assert_eq!(
        report.per_redaction[0], report.per_redaction[1],
        "zwei deckungsgleiche Regionen müssen dasselbe zählen: {:?}",
        report.per_redaction
    );
    assert_eq!(
        report.per_redaction[0],
        SECRET.len(),
        "die genaue Hülle trifft genau ihre Zeichen"
    );
    assert!(
        report.per_redaction[2] >= SECRET.len(),
        "der größere Bereich trifft mindestens dieselben: {:?}",
        report.per_redaction
    );
    assert_eq!(
        report.removed_glyphs, report.per_redaction[2],
        "der Strom verliert die Vereinigung, jedes Zeichen genau einmal"
    );
    assert!(leaks(&doc.bytes(), SECRET).is_empty());
}

/// Die Schwärzung eines mehrfach platzierten Formulars trifft weiterhin
/// **alle** Platzierungen — auch mit Zwischenspeicher und Vorauswahl.
#[test]
fn ein_mehrfach_platziertes_formular_wird_ganz_geschwaerzt() {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    let form_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let form = form_with_resources(
        &mut doc,
        form_resources,
        &format!("BT /F1 10 Tf 20 10 Td ({SECRET}) Tj ET\n"),
    );
    doc.set_resources(dictionary! { "XObject" => dictionary! { "Fx" => form } });
    doc.set_content(
        (0..10)
            .map(|k| format!("q 1 0 0 1 0 {} cm /Fx Do Q\n", 700 - k * 30))
            .collect::<String>(),
    );

    let redactions = redactions_for_secret(&doc);
    assert_eq!(redactions.len(), 10, "zehn Platzierungen, zehn Treffer");

    let report = PdfRedactor::new()
        .apply_with_report(&mut doc.doc, &redactions)
        .expect("schwärzbar");
    assert_eq!(
        report.removed_glyphs,
        SECRET.len(),
        "der Strom wird einmal neu geschrieben"
    );
    assert!(leaks(&doc.bytes(), SECRET).is_empty());
}

// ---------------------------------------------------------------------------
// Befund 3 — ein indirektes /Subtype ist trotzdem ein /Subtype
// ---------------------------------------------------------------------------

/// `/Subtype 9 0 R → /Image` verhält sich wie `/Subtype /Image`.
///
/// Vorher wies der Torwächter im Interpreter das XObject ab („kein bekanntes
/// /Subtype“), meldete eine Deckungslücke (Rückgabewert 3) und schwärzte kein
/// einziges Bild — obwohl PDF 32000-1 (7.3.10) jedem Wert eine indirekte
/// Referenz erlaubt. Die Auflösung, die dafür in `ops::image_xobject` stand,
/// kam wegen dieses Torwächters nie zum Zug.
///
/// Geprüft wird nicht „keine Warnung“, sondern **dieselbe** Antwort: die
/// Warnung, dass ein Rasterbild ohne OCR nicht gelesen wird, gehört in beiden
/// Fällen dazu.
#[test]
fn ein_indirektes_subtype_wird_aufgeloest() {
    let lauf = |indirect: bool| {
        let mut doc = Doc::new();
        let font = helvetica(&mut doc);
        let subtype: Object = if indirect {
            Object::Reference(doc.add(Object::Name(b"Image".to_vec())))
        } else {
            Object::Name(b"Image".to_vec())
        };
        let pixels: Vec<u8> = (0..64u16).map(|v| v as u8).collect();
        let image = doc.add(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => subtype,
                "Width" => 8_i64,
                "Height" => 8_i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8_i64,
            },
            pixels,
        ));
        doc.set_resources(dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Im0" => image },
        });
        doc.set_content("q 200 0 0 200 100 500 cm /Im0 Do Q\n");

        let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
        let report = PdfRedactor::new()
            .apply_with_report(
                &mut doc.doc,
                &[blackout(Rect::new(100.0, 500.0, 300.0, 700.0))],
            )
            .expect("schwärzbar");
        (scan.warnings, report.redacted_images, report.warnings)
    };

    let direkt = lauf(false);
    let indirekt = lauf(true);
    assert_eq!(
        direkt.1, 1,
        "die Vorlage muss überhaupt ein schwärzbares Bild haben"
    );
    assert!(
        direkt.0.is_empty(),
        "das direkte /Subtype darf keine Deckungslücke melden: {:?}",
        direkt.0
    );
    assert_eq!(
        direkt, indirekt,
        "ein indirektes /Subtype muss dieselbe Antwort geben wie ein direktes"
    );
}

/// Dasselbe für `/Subtype 9 0 R → /Form`: der Text darin wird gefunden.
#[test]
fn ein_indirektes_subtype_form_wird_durchsucht() {
    let mut doc = Doc::new();
    let font = helvetica(&mut doc);
    let form_resources = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let subtype = doc.add(Object::Name(b"Form".to_vec()));
    let form = doc.add(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => Object::Reference(subtype),
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => form_resources,
        },
        format!("BT /F1 12 Tf 20 700 Td ({SECRET}) Tj ET\n").into_bytes(),
    ));
    doc.set_resources(dictionary! { "XObject" => dictionary! { "Fx" => form } });
    doc.set_content("q /Fx Do Q\n");

    let scan = scan_page(&doc.doc, doc.page_id).expect("lesbar");
    assert!(scan.warnings.is_empty(), "{:?}", scan.warnings);
    assert_eq!(shown_texts(&scan), vec![SECRET.to_string()]);

    let redactions = redactions_for_secret(&doc);
    assert_eq!(redactions.len(), 1);
    PdfRedactor::new()
        .apply_with_report(&mut doc.doc, &redactions)
        .expect("schwärzbar");
    assert!(leaks(&doc.bytes(), SECRET).is_empty());
}
