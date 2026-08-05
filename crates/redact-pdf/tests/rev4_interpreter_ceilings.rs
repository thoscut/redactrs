//! Was der Interpreter je Platzierung **nicht** noch einmal tun darf — und
//! woran seine Zwischenspeicher-Decken maschinell zu erkennen sind.
//!
//! Drei Befunde, alle aus derselben Familie: eine kleine Datei bringt den
//! Scanner dazu, dieselbe Arbeit immer wieder zu tun oder dasselbe Ergebnis
//! immer wieder abzulegen.
//!
//! 1. **Der Grafikzustand trug die Schrift als Wert.** Jedes `Tf` und jedes
//!    `q` kopierte damit die vollständige `/ToUnicode`-Zuordnung. Gemessen
//!    (Release, Schrift mit 125 000 Einträgen): 3,8 ms je `Tf`, 4,1 ms je
//!    `q Q`, linear wachsend — bei einem Aufwandskonto, das eine Million
//!    Operationen zulässt. Der Zwischenspeicher aus v0.4.0 half hier nicht: er
//!    verhindert das erneute *Parsen*, nicht das *Kopieren*.
//! 2. **Dieselbe Schrift wurde je Ressourcennamen neu geparst.** Ein
//!    Verzeichnis mit 40 Namen auf dasselbe Schriftobjekt belegte 279 MB und
//!    brauchte 2,38 s; die Datei wuchs dafür um 10 Byte je Name.
//! 3. **`declare_forms` war je Strom gesperrt, arbeitete aber je
//!    Verzeichnis.** *n* Formulare ohne eigenes `/Resources`, die sich ein
//!    `/XObject` mit *n* Einträgen teilen, ergaben *n*² Auflösungen: gemessen
//!    4,65 s für 4 000 Formulare aus einer Datei von 564 kB, 21,9 s für 8 000.
//!
//! ## Warum hier fast keine Uhr steht
//!
//! Wie in `placement_cost.rs` wird gemessen, was an der **Datei** hängt und
//! nicht am Rechner: [`redact_pdf::content::ScanEffort`]. Dazu sind drei
//! Zähler da — wie oft ein Strom ausgepackt, wie oft ein Verzeichnis übersetzt,
//! wie oft ein **Schriftobjekt geparst** und wie oft eine `/XObject`-Liste
//! angeboten wurde. Diese Zahlen sind deterministisch.
//!
//! Genau **ein** Test benutzt trotzdem eine Uhr
//! ([`viele_tf_kosten_nicht_die_ganze_tabelle`]): der Preis eines `Tf` ist
//! reine Rechenzeit und schlägt sich in keinem Zähler nieder. Seine Schranke
//! ist deshalb mit dem Faktor 50 gewählt — sie greift bei dem Fehler, den sie
//! meint, und flattert nicht mit der Fremdlast der Maschine.
//!
//! ## Und die Decken
//!
//! Beide Zwischenspeicher-Decken waren vorher von **keinem** Test gehalten:
//! auf `usize::MAX` gesetzt blieb die ganze Suite grün. Sie sind aber
//! maschinell prüfbar, und zwar von innen heraus: **über** der Decke wird
//! nichts mehr abgelegt, also fällt die Arbeit wieder je Platzierung an. Eine
//! Datei, die die Decke reißt, liefert deshalb mehr Auspack- bzw.
//! Parsevorgänge, als sie Objekte hat. Ohne Decke wäre es genau die
//! Objektzahl — die Tests unten gehen rot.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_pdf::{scan_page, ShowItem};

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

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

    fn scan(&self) -> redact_pdf::ScanResult {
        scan_page(&self.doc, self.page_id).expect("lesbar")
    }
}

/// Eine Schrift mit einer **großen**, aber winzig notierten `/ToUnicode`.
///
/// `1 beginbfrange <0000> <FFFF> <2000> endbfrange` sind vierzig Byte und
/// ergeben 65 536 Einträge. Genau das ist die Bauart, um die es geht: die
/// Datei bleibt klein, die Tabelle im Speicher nicht. Sie bleibt dabei
/// **unter** `MAX_TO_UNICODE_BYTES` und wird deshalb auch wirklich benutzt.
fn font_with_range(doc: &mut Doc, entries: u32, base: u32) -> ObjectId {
    let hi = entries.saturating_sub(1).min(0xFFFF);
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Test def /CMapType 2 def\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
         1 beginbfrange <0000> <{hi:04X}> <{base:04X}> endbfrange\n\
         endcmap CMapName currentdict /CMap defineresource pop end end\n"
    );
    let cmap_id = doc.add(Stream::new(dictionary! {}, cmap.into_bytes()));
    doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
        "ToUnicode" => cmap_id,
    })
}

/// Eine Helvetica ohne jede Tabelle — sie wiegt nichts.
fn helvetica(doc: &mut Doc) -> ObjectId {
    doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    })
}

fn form(doc: &mut Doc, resources: Option<ObjectId>, content: &str) -> ObjectId {
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    };
    if let Some(resources) = resources {
        dict.set("Resources", resources);
    }
    doc.add(Stream::new(dict, content.as_bytes().to_vec()))
}

/// Der gesetzte Text je Datensatz.
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
// Befund 1 — der Grafikzustand darf die Schrift nicht kopieren
// ---------------------------------------------------------------------------

/// `Tf` und `q` kosten nicht die Größe der Schrifttabelle.
///
/// Der **einzige** Test hier mit einer Uhr, und der Grund steht im Modulkopf:
/// ein `Tf` schlägt sich in keinem Zähler nieder, sein Preis ist reine
/// Rechenzeit. Die Schranke ist deshalb grob.
///
/// Rechnung zur Schranke: vorher kostete ein `Tf` bei dieser Tabelle rund
/// 2 ms (Release), 20 000 davon also rund 40 s — in einem Debug-Lauf ein
/// Vielfaches. Nachher ist es ein Zeigerkopieren, und der ganze Scan bleibt
/// unter einer Sekunde. Zehn Sekunden liegen dazwischen mit Faktor 50 in
/// beide Richtungen.
#[test]
fn viele_tf_kosten_nicht_die_ganze_tabelle() {
    let mut doc = Doc::new();
    let font = font_with_range(&mut doc, 0x10000, 0x2000);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    // 20 000 `Tf` und 20 000 `q`/`Q` — jedes davon fasste vorher den ganzen
    // Grafikzustand an, in dem die Schrift als Wert stand.
    doc.set_content(format!(
        "BT\n{}ET\n{}",
        "/F1 12 Tf\n".repeat(20_000),
        "q Q\n".repeat(20_000)
    ));

    let start = std::time::Instant::now();
    let scan = doc.scan();
    let dauer = start.elapsed();

    assert_eq!(scan.effort.parsed_fonts, 1, "eine Schrift, einmal geparst");
    assert!(
        dauer.as_secs_f64() < 10.0,
        "40 000 Zustandsoperationen an einer Schrift mit 65 536 Einträgen \
         dauerten {:.1} s — der Grafikzustand kopiert die Tabelle wieder",
        dauer.as_secs_f64()
    );
}

/// Und das Ergebnis bleibt dasselbe: die geteilte Schrift dekodiert weiter.
#[test]
fn die_geteilte_schrift_dekodiert_weiter_richtig() {
    let mut doc = Doc::new();
    // Code 0x41 ('A') → 0x2000 + 0x41.
    let font = font_with_range(&mut doc, 0x10000, 0x2000);
    doc.set_resources(dictionary! { "Font" => dictionary! { "F1" => font } });
    doc.set_content("BT /F1 12 Tf q Q 20 20 Td (A) Tj ET\n");

    let scan = doc.scan();
    let erwartet: String = char::from_u32(0x2000 + 0x41).expect("Zeichen").into();
    assert_eq!(shown_texts(&scan), vec![erwartet]);
}

// ---------------------------------------------------------------------------
// Befund 2 — dieselbe Schrift wird einmal geparst
// ---------------------------------------------------------------------------

/// Dreihundert Ressourcennamen auf **ein** Schriftobjekt: einmal geparst.
///
/// Der Zwischenspeicher lag auf dem Ressourcen*verzeichnis*, nicht auf dem
/// Schrift*objekt* — innerhalb eines Verzeichnisses griff er also gar nicht.
/// Gemessen wuchs der Spitzenspeicher linear mit der Zahl der Namen (40 Namen
/// → 279 MB), während die Datei je Name um zehn Byte wuchs.
#[test]
fn dieselbe_schrift_unter_vielen_namen_wird_einmal_geparst() {
    let mut doc = Doc::new();
    let font = font_with_range(&mut doc, 0x10000, 0x2000);
    let mut fonts = Dictionary::new();
    for k in 0..300 {
        fonts.set(format!("F{k}"), font);
    }
    doc.set_resources(dictionary! { "Font" => fonts });
    doc.set_content("BT /F0 12 Tf 20 20 Td (A) Tj /F299 12 Tf (A) Tj ET\n");

    let scan = doc.scan();
    assert_eq!(
        scan.effort.parsed_fonts, 1,
        "dreihundert Namen, ein Schriftobjekt — und trotzdem {} Parsevorgänge",
        scan.effort.parsed_fonts
    );
    // Und beide Namen liefern denselben, richtigen Text.
    let erwartet: String = char::from_u32(0x2000 + 0x41).expect("Zeichen").into();
    assert_eq!(shown_texts(&scan), vec![erwartet.clone(), erwartet]);
}

/// Auch über Verzeichnisgrenzen hinweg: dasselbe Objekt, ein Parsevorgang.
#[test]
fn dieselbe_schrift_in_vielen_verzeichnissen_wird_einmal_geparst() {
    let mut doc = Doc::new();
    let font = font_with_range(&mut doc, 0x10000, 0x2000);
    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    for k in 0..50 {
        // Jedes Formular ein eigenes `/Resources`-Objekt, aber dieselbe Schrift.
        let resources = doc.add(dictionary! {
            "Font" => dictionary! { "F1" => font },
            "Marker" => k as i64,
        });
        let f = form(&mut doc, Some(resources), "BT /F1 12 Tf 1 1 Td (A) Tj ET\n");
        xobjects.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
    }
    doc.set_resources(dictionary! { "XObject" => xobjects });
    doc.set_content(content);

    let scan = doc.scan();
    assert_eq!(
        scan.effort.loaded_font_maps, 51,
        "einundfünfzig verschiedene Verzeichnisse, jedes einmal übersetzt"
    );
    assert_eq!(
        scan.effort.parsed_fonts, 1,
        "aber nur ein Schriftobjekt — und das wird einmal geparst"
    );
    assert_eq!(scan.shows.len(), 50, "jede Platzierung setzt ihren Text");
}

/// **Die Gegenprobe zur Teilung:** dieselbe Objekt-Id heißt dieselbe Schrift,
/// aber derselbe *Name* heißt gar nichts.
///
/// Ein Formular ohne eigenes `/Resources` wird aus zwei Umgebungen gezeichnet,
/// die unter `/F1` **verschiedene** Schriftobjekte anbieten. Die beiden
/// `/ToUnicode`-Zuordnungen bilden dieselben Codes auf verschiedene Zeichen ab.
/// Ein Zwischenspeicher, der auf dem Namen statt auf dem Objekt läge, gäbe hier
/// zweimal dieselbe Antwort — und die Schwärzung suchte im zweiten
/// Zusammenhang nach dem falschen Text.
#[test]
fn derselbe_name_auf_verschiedene_objekte_bleibt_verschieden() {
    for (a_zuerst, erwartet) in [
        (true, ["\u{2041}", "\u{3041}"]),
        (false, ["\u{3041}", "\u{2041}"]),
    ] {
        let mut doc = Doc::new();
        let font_a = font_with_range(&mut doc, 0x10000, 0x2000);
        let font_b = font_with_range(&mut doc, 0x10000, 0x3000);
        let geteilt = form(&mut doc, None, "BT /F1 12 Tf 20 20 Td (A) Tj ET\n");

        let res_a = doc.add(dictionary! {
            "Font" => dictionary! { "F1" => font_a },
            "XObject" => dictionary! { "S" => geteilt },
        });
        let res_b = doc.add(dictionary! {
            "Font" => dictionary! { "F1" => font_b },
            "XObject" => dictionary! { "S" => geteilt },
        });
        let a = form(&mut doc, Some(res_a), "q 1 0 0 1 0 700 cm /S Do Q\n");
        let b = form(&mut doc, Some(res_b), "q 1 0 0 1 0 600 cm /S Do Q\n");

        doc.set_resources(dictionary! {
            "XObject" => dictionary! { "A" => a, "B" => b },
        });
        doc.set_content(if a_zuerst {
            "q /A Do Q q /B Do Q\n"
        } else {
            "q /B Do Q q /A Do Q\n"
        });

        let scan = doc.scan();
        assert_eq!(
            shown_texts(&scan),
            erwartet.map(String::from).to_vec(),
            "derselbe Strom, zwei geerbte Verzeichnisse — zwei Ergebnisse \
             (A zuerst: {a_zuerst})"
        );
        assert_eq!(scan.effort.parsed_fonts, 2, "zwei Schriftobjekte");
    }
}

// ---------------------------------------------------------------------------
// Befund 3 — `declare_forms` hängt am Verzeichnis, nicht am Strom
// ---------------------------------------------------------------------------

/// 200 Formulare erben dasselbe `/XObject` mit 200 Einträgen: **eine**
/// Durchsicht.
///
/// Vorher waren es 201 — je Strom eine, und jede ging alle 200 Einträge durch
/// und löste sie auf. Das ist quadratisch: gemessen 4,65 s für 4 000
/// Formulare aus 564 kB, 21,9 s für 8 000, gegen 0,027 s bzw. 0,071 s danach.
/// Von keinem Aufwandskonto gesehen, weil die Arbeit vor der ersten gezählten
/// Operation anfällt.
#[test]
fn ein_geerbtes_xobject_verzeichnis_wird_einmal_angeboten() {
    let mut doc = Doc::new();
    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    let mut ids = Vec::new();
    for k in 0..200 {
        // Ohne eigenes `/Resources`: jedes dieser Formulare erbt das
        // 200-Einträge-Verzeichnis der Seite.
        let f = form(&mut doc, None, "0 0 1 1 re f\n");
        xobjects.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
        ids.push(f);
    }
    doc.set_resources(dictionary! { "XObject" => xobjects });
    doc.set_content(content);

    let scan = doc.scan();
    assert_eq!(
        scan.effort.declared_resources, 1,
        "ein Verzeichnis, eine Durchsicht — nicht eine je Strom"
    );
    // Und die Auskunft bleibt vollständig: alle 200 Formulare sind angeboten.
    assert_eq!(scan.declared_forms.len(), 200);
    for id in &ids {
        assert!(
            scan.declared_forms.contains_key(id),
            "Formular {id:?} fehlt in der Liste der angebotenen"
        );
    }
}

/// Auch wenn jedes Formular ein **eigenes** `/Resources` mitbringt, die
/// `/XObject`-Liste darin aber dasselbe Objekt ist.
#[test]
fn ein_geteiltes_xobject_objekt_wird_einmal_angeboten() {
    let mut doc = Doc::new();
    // Erst die Formulare anlegen, dann die geteilte Liste füllen.
    let mut xobjects = Dictionary::new();
    let liste_id = doc.doc.new_object_id();
    let mut content = String::new();
    for k in 0..100 {
        let eigene = doc.add(dictionary! { "XObject" => Object::Reference(liste_id) });
        let f = form(&mut doc, Some(eigene), "0 0 1 1 re f\n");
        xobjects.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
    }
    doc.doc
        .objects
        .insert(liste_id, Object::Dictionary(xobjects));
    doc.set_resources(dictionary! { "XObject" => Object::Reference(liste_id) });
    doc.set_content(content);

    let scan = doc.scan();
    assert_eq!(
        scan.effort.declared_resources, 1,
        "hundertundeine Umgebung, aber nur eine `/XObject`-Liste"
    );
    assert_eq!(scan.declared_forms.len(), 100);
}

/// Die Gegenrichtung: wirklich verschiedene Listen werden auch einzeln
/// angeboten. Die Sperre darf nichts verschlucken.
#[test]
fn verschiedene_xobject_verzeichnisse_werden_einzeln_angeboten() {
    let mut doc = Doc::new();
    let innen: Vec<ObjectId> = (0..5)
        .map(|_| form(&mut doc, None, "0 0 1 1 re f\n"))
        .collect();
    let mut aussen = Dictionary::new();
    let mut content = String::new();
    for (k, inner) in innen.iter().enumerate() {
        // Jede Umgebung eine eigene Liste mit einem eigenen Eintrag.
        let liste = doc.add(dictionary! { "In" => *inner });
        let eigene = doc.add(dictionary! { "XObject" => liste });
        let f = form(&mut doc, Some(eigene), "q /In Do Q\n");
        aussen.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
    }
    doc.set_resources(dictionary! { "XObject" => aussen });
    doc.set_content(content);

    let scan = doc.scan();
    assert_eq!(
        scan.effort.declared_resources, 6,
        "die Seite und fünf eigene Listen"
    );
    // Fünf äußere und fünf innere Formulare sind angeboten worden.
    assert_eq!(scan.declared_forms.len(), 10);
}

// ---------------------------------------------------------------------------
// Befund 4 — die beiden Decken, von innen geprüft
// ---------------------------------------------------------------------------
//
// Beide arbeiten nach demselben Muster: **über** der Decke wird nichts mehr
// abgelegt, also fällt die Arbeit wieder je Platzierung an. Wer die Decke
// aufhebt, senkt damit die Zahl der Vorgänge auf die Zahl der Objekte — und
// genau das prüfen die drei Tests hier.

/// Die Operationsdecke hält: über ihr wird wieder je Platzierung ausgepackt.
///
/// Zwanzig Formulare zu je 6 000 Operationen sind 120 000 — mehr als die Decke
/// von 100 000. Jedes wird zweimal gezeichnet. Was noch hineinpasste, wird
/// einmal ausgepackt; der Rest zweimal.
///
/// Mit `MAX_CACHED_OPERATIONS = usize::MAX` sind es genau zwanzig, und dieser
/// Test geht rot.
#[test]
fn die_operationsdecke_haelt() {
    let mut doc = Doc::new();
    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    let rumpf = "0 0 1 1 re f\n".repeat(3_000); // 6 000 Operationen
    for k in 0..20 {
        let eigene = doc.add(dictionary! { "Marker" => k as i64 });
        let f = form(&mut doc, Some(eigene), &rumpf);
        xobjects.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
    }
    // Zweimal die ganze Reihe — die zweite Runde lebt vom Zwischenspeicher.
    let content = content.repeat(2);
    doc.set_resources(dictionary! { "XObject" => xobjects });
    doc.set_content(content);

    let scan = doc.scan();
    assert!(
        scan.effort.decoded_streams > 20,
        "zwanzig Formularobjekte mit zusammen 120 000 Operationen passen nicht \
         unter eine Decke von 100 000 — es müssten also mehr als zwanzig \
         Auspackvorgänge anfallen, gezählt wurden {}. Die Decke ist \
         wirkungslos.",
        scan.effort.decoded_streams
    );
    assert!(
        scan.effort.decoded_streams < 40,
        "und der Zwischenspeicher muss trotzdem greifen: {} von höchstens 40",
        scan.effort.decoded_streams
    );
}

/// Die Operationsdecke sieht auch das **Ressourcenverzeichnis**.
///
/// Sechzig Formulare mit *null* Operationen, aber je 2 000
/// `/Resources`-Einträgen. Zählte die Decke nur Operationen, kosteten sie
/// zusammen null und würden **alle** behalten — 60 × 2 000 Einträge als
/// Dictionary-Kopie, ohne dass die Decke etwas davon sähe. Gemessen kostete
/// das 12,8 kB je Strom und wuchs linear weiter: 5 000 solcher Ströme legten
/// 62,6 MB an, wo der Zähler „0“ ablas.
///
/// Mit einer Decke, die nur `operations().len()` zählt, sind es genau sechzig,
/// und dieser Test geht rot.
#[test]
fn die_operationsdecke_sieht_das_ressourcenverzeichnis() {
    let mut doc = Doc::new();
    let helv = helvetica(&mut doc);
    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    for k in 0..60 {
        let mut fett = Dictionary::new();
        for j in 0..2_000 {
            fett.set(format!("P{j}"), helv);
        }
        let eigene = doc.add(dictionary! { "Properties" => fett, "Marker" => k as i64 });
        let f = form(&mut doc, Some(eigene), "");
        xobjects.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
    }
    let content = content.repeat(2);
    doc.set_resources(dictionary! { "XObject" => xobjects });
    doc.set_content(content);

    let scan = doc.scan();
    assert!(
        scan.effort.decoded_streams > 60,
        "sechzig Ströme mit null Operationen, aber je 2 000 \
         Ressourceneinträgen: die Decke muss das Verzeichnis mitzählen, sonst \
         behält sie alle sechzig umsonst. Gezählt wurden {} Auspackvorgänge.",
        scan.effort.decoded_streams
    );
}

/// Die Schriftendecke hält: über ihr wird wieder je Verzeichnis neu geparst.
///
/// Sieben Schriften mit je 65 536 Einträgen sind 458 752 — mehr als die Decke
/// von 400 000 Einträgen. Jedes der beiden Verzeichnisse nennt alle sieben,
/// und beide werden gezeichnet.
///
/// Mit `MAX_CACHED_FONT_ENTRIES = usize::MAX` sind es genau sieben
/// Parsevorgänge, und dieser Test geht rot. Die frühere Decke zählte
/// *Verzeichnisse* (16) und hätte hier ohnehin nichts gemerkt: es sind drei.
#[test]
fn die_schriftendecke_haelt() {
    let mut doc = Doc::new();
    let fonts: Vec<ObjectId> = (0..7)
        .map(|k| font_with_range(&mut doc, 0x10000, 0x2000 + k * 0x100))
        .collect();
    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    for k in 0..2 {
        let mut font_dict = Dictionary::new();
        for (j, f) in fonts.iter().enumerate() {
            font_dict.set(format!("F{j}"), *f);
        }
        let eigene = doc.add(dictionary! { "Font" => font_dict, "Marker" => k as i64 });
        let f = form(&mut doc, Some(eigene), "BT /F0 12 Tf 1 1 Td (A) Tj ET\n");
        xobjects.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
    }
    doc.set_resources(dictionary! { "XObject" => xobjects });
    doc.set_content(content);

    let scan = doc.scan();
    assert!(
        scan.effort.parsed_fonts > 7,
        "sieben Schriften zu je 65 536 Einträgen passen nicht unter eine Decke \
         von 400 000 Einträgen — über der Decke muss wieder geparst werden. \
         Gezählt wurden {} Parsevorgänge; die Decke ist wirkungslos.",
        scan.effort.parsed_fonts
    );
    // Und das Ergebnis bleibt richtig, gleich auf welcher Seite der Decke.
    let erwartet: String = char::from_u32(0x2000 + 0x41).expect("Zeichen").into();
    assert_eq!(shown_texts(&scan), vec![erwartet.clone(), erwartet]);
}

/// Unter der Decke wird dagegen nichts doppelt geparst — sonst wäre der Test
/// oben auch grün, wenn der Zwischenspeicher gar nicht griffe.
#[test]
fn unter_der_schriftendecke_wird_nichts_doppelt_geparst() {
    let mut doc = Doc::new();
    let fonts: Vec<ObjectId> = (0..3)
        .map(|k| font_with_range(&mut doc, 0x10000, 0x2000 + k * 0x100))
        .collect();
    let mut xobjects = Dictionary::new();
    let mut content = String::new();
    for k in 0..8 {
        let mut font_dict = Dictionary::new();
        for (j, f) in fonts.iter().enumerate() {
            font_dict.set(format!("F{j}"), *f);
        }
        let eigene = doc.add(dictionary! { "Font" => font_dict, "Marker" => k as i64 });
        let f = form(&mut doc, Some(eigene), "BT /F0 12 Tf 1 1 Td (A) Tj ET\n");
        xobjects.set(format!("X{k}"), f);
        content.push_str(&format!("q /X{k} Do Q\n"));
    }
    doc.set_resources(dictionary! { "XObject" => xobjects });
    doc.set_content(content.repeat(3));

    let scan = doc.scan();
    assert_eq!(
        scan.effort.parsed_fonts, 3,
        "drei Schriftobjekte zu zusammen 196 608 Einträgen bleiben unter der \
         Decke — acht Verzeichnisse und drei Runden ändern daran nichts"
    );
}

// ---------------------------------------------------------------------------
// Gegenprobe: was heute hält, muss weiter halten
// ---------------------------------------------------------------------------

/// Ein Aufwandskonto gilt je Seiten-Scan; zwei Läufe kosten dasselbe.
///
/// Der Zwischenspeicher darf nicht über den Scan hinaus leben — sonst wüchse
/// er über ein Dokument hinweg auf, und die Decke gälte nur noch für die erste
/// Seite.
#[test]
fn zwei_scans_derselben_seite_kosten_dasselbe() {
    let mut doc = Doc::new();
    let font = font_with_range(&mut doc, 0x1000, 0x2000);
    let eigene = doc.add(dictionary! { "Font" => dictionary! { "F1" => font } });
    let f = form(&mut doc, Some(eigene), "BT /F1 12 Tf 1 1 Td (A) Tj ET\n");
    doc.set_resources(dictionary! { "XObject" => dictionary! { "X" => f } });
    doc.set_content("q /X Do Q\n".repeat(10));

    let erst = doc.scan().effort;
    let zweit = doc.scan().effort;
    assert_eq!(erst, zweit, "derselbe Scan, derselbe Aufwand");
    assert_eq!(erst.parsed_fonts, 1);
    assert_eq!(erst.decoded_streams, 1);
}

/// Die dokumentierte Fächerung wird weiterhin abgelehnt.
///
/// Sieben Ebenen, in denen jedes Formular das nächste achtmal zeichnet, sind
/// 8⁷ Durchläufe aus wenigen hundert Byte. Weder der Schriften- noch der
/// Strom-Zwischenspeicher darf daran etwas ändern: sie sparen das Auspacken,
/// nicht das Auswerten.
#[test]
fn die_faecherung_wird_weiterhin_abgelehnt() {
    let mut doc = Doc::new();
    let mut unten = form(&mut doc, None, "0 0 1 1 re f\n");
    for _ in 0..7 {
        let liste = doc.add(dictionary! { "N" => unten });
        let eigene = doc.add(dictionary! { "XObject" => liste });
        unten = form(&mut doc, Some(eigene), &"q /N Do Q\n".repeat(8));
    }
    doc.set_resources(dictionary! { "XObject" => dictionary! { "T" => unten } });
    doc.set_content("q /T Do Q\n");

    let fehler = scan_page(&doc.doc, doc.page_id).expect_err("muss abgelehnt werden");
    let text = fehler.to_string();
    assert!(
        text.contains("vervielfacht"),
        "die Ablehnung muss die Fächerung benennen: {text}"
    );
}

/// Eine `/ToUnicode` **über** [`redact_pdf::encoding::MAX_TO_UNICODE_BYTES`]
/// wird verworfen — und belastet dann auch die Schriftendecke nicht.
///
/// Sonst hätte eine einzige abgelehnte Bombe den Zwischenspeicher für die
/// ganze Seite dichtgemacht, und jede weitere Schrift wäre je Platzierung neu
/// geparst worden: aus einer abgewehrten Bombe würde eine langsame Seite.
#[test]
fn eine_verworfene_bombe_belegt_die_schriftendecke_nicht() {
    let mut doc = Doc::new();
    // 300 000 Einträge à 259 Byte reißen die 32-MB-Grenze.
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Bombe def /CMapType 2 def\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
    );
    for k in 0..5u32 {
        cmap.push_str(&format!(
            "1 beginbfrange <0000> <FFFF> <{:04X}> endbfrange\n",
            0x2000 + k
        ));
    }
    cmap.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");
    let cmap_id = doc.add(Stream::new(dictionary! {}, cmap.into_bytes()));
    let bombe = doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
        "ToUnicode" => cmap_id,
    });
    let harmlos = font_with_range(&mut doc, 0x10000, 0x3000);

    let eigene = doc.add(dictionary! {
        "Font" => dictionary! { "F1" => bombe, "F2" => harmlos },
    });
    let f = form(
        &mut doc,
        Some(eigene),
        "BT /F2 12 Tf 1 1 Td (A) Tj /F1 12 Tf (A) Tj ET\n",
    );
    doc.set_resources(dictionary! { "XObject" => dictionary! { "X" => f } });
    doc.set_content("q /X Do Q\n".repeat(4));

    let scan = doc.scan();
    assert_eq!(
        scan.effort.parsed_fonts, 2,
        "vier Platzierungen, zwei Schriftobjekte — die verworfene Bombe darf \
         die Decke nicht füllen"
    );
    // Die harmlose Schrift liest weiter, die Bombe fällt auf WinAnsi zurück.
    let harmloses: String = char::from_u32(0x3000 + 0x41).expect("Zeichen").into();
    assert_eq!(
        shown_texts(&scan),
        std::iter::repeat_n([harmloses, "A".to_string()], 4)
            .flatten()
            .collect::<Vec<_>>(),
        "je Platzierung ein `Tj` mit der harmlosen Schrift und eins mit der Bombe"
    );
}
