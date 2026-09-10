//! Gegenprüfung Fix-Runde 5 (Q1): der Trägerlauf hat **keine Tiefengrenze**
//! mehr. Endet er trotzdem — bei Zyklen, bei Fächern, bei langen Ketten und
//! bei tief eingebetteten *direkten* Bäumen?
//!
//! Gedeckelt ist der Lauf allein durch die Besuchsmenge (je Objekt einmal)
//! und, nach oben, durch `Limits::max_parsed_bytes` beim Laden. Diese Datei
//! prüft die untere Hälfte: dass keine der Formen hängen bleibt und dass der
//! Klartext dahinter fällt.
//!
//! Der Speicher wird hier nicht gemessen — das gehört in einen Kindprozess
//! (VmHWM); die Messreihe steht im Bericht der Gegenprüfung.

mod common;

use std::time::{Duration, Instant};

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata};

/// Großzügig: gemessen liegen alle Fälle hier unter einer Sekunde. Eine
/// Laufzeit über einer Minute an einer kleinen Datei wäre ein Befund.
const FRIST: Duration = Duration::from_secs(60);

/// Läuft in einem eigenen Faden, damit ein **Hängen** des Laufs als roter
/// Test endet und nicht als Test, der nie zurückkommt: eine Frist, die erst
/// nach `strip_metadata` geprüft wird, prüft nichts.
fn strip_mit_frist(bytes: &[u8], was: &str) -> (Vec<u8>, Duration) {
    let (sender, empfang) = std::sync::mpsc::channel();
    let eigene = bytes.to_vec();
    let start = Instant::now();
    std::thread::spawn(move || {
        let mut doc = load_from_bytes(&eigene).expect("PDF ladbar");
        strip_metadata(&mut doc);
        let _ = sender.send(save_to_bytes(&doc).expect("Speichern"));
    });
    match empfang.recv_timeout(FRIST) {
        Ok(out) => (out, start.elapsed()),
        Err(_) => panic!("{was}: der Lauf ist nach {FRIST:?} nicht zurück — er endet nicht"),
    }
}

#[track_caller]
fn kein_leck(out: &[u8], was: &str) {
    let hits = leaks(out, SECRET);
    assert!(
        hits.is_empty(),
        "{was}: noch in der Ausgabe:\n{}",
        hits.join("\n")
    );
}

fn mit_annots(d: &mut Doc, first: ObjectId) {
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(first)]));
}

/// Ein `/Popup`-Zyklus: die Notiz zeigt auf das Popup, das Popup zurück auf
/// die Notiz — und der Klartext steht **nur** im Popup. Der Lauf muss
/// `/Popup` folgen *und* enden.
///
/// Mutationsnachweis: `b"Popup"` aus `ANNOTATION_LINK_KEYS` streichen — dann
/// bleibt `/Contents` des Popups stehen und der Test ist rot. Zweiter
/// Mutationsnachweis: die Besuchsmenge unwirksam machen (`if !visited.insert(id)`
/// → `if false`) — dann endet der Lauf nicht.
#[test]
fn ein_popup_zyklus_endet_und_das_popup_wird_bereinigt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let notiz = d.doc.new_object_id();
    let popup = d.doc.new_object_id();
    d.doc.objects.insert(
        notiz,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Popup" => Object::Reference(popup),
        }),
    );
    d.doc.objects.insert(
        popup,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Popup",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            // Zurück auf die Notiz: der Zyklus.
            "Popup" => Object::Reference(notiz),
            "Parent" => Object::Reference(notiz),
            "Contents" => Object::string_literal(format!("Notiz {SECRET}")),
        }),
    );
    mit_annots(&mut d, notiz);
    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (out, _) = strip_mit_frist(&bytes, "/Popup-Zyklus");
    kein_leck(&out, "/Popup-Zyklus");
}

/// `collect_references` läuft ohne Tiefengrenze: ein Objekt, das nur hinter
/// 70 verschachtelten Arrays hängt, ist **erreichbar** und darf nicht
/// weggeräumt werden.
///
/// Mutationsnachweis: in `document.rs` die Tiefengrenze wieder einziehen
/// (Zähler mit `if depth > 64 { continue; }`) — dann ist das Objekt weg und
/// der Test ist rot.
#[test]
fn ein_objekt_hinter_siebzig_arrays_bleibt_erreichbar() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let tief = d.add(Object::Dictionary(dictionary! {
        "Merkmal" => Object::string_literal("bleibt"),
    }));
    let mut nest = Object::Array(vec![Object::Reference(tief)]);
    for _ in 0..69 {
        nest = Object::Array(vec![nest]);
    }
    d.page_dict_set("Zusatz", nest);
    let bytes = d.finish();

    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let wieder = load_from_bytes(&out).expect("Ausgabe lädt");
    assert!(
        wieder.objects.contains_key(&tief),
        "das Objekt hinter 70 Arrays wurde als unerreichbar gelöscht"
    );
}

/// Zwei Objekte, die sich gegenseitig als `/Parent` **und** `/Kids` tragen.
#[test]
fn ein_gegenseitiger_parent_kids_zyklus_endet() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let a = d.doc.new_object_id();
    let b = d.doc.new_object_id();
    d.doc.objects.insert(
        a,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Parent" => Object::Reference(b),
            "Kids" => vec![Object::Reference(b)],
            "T" => Object::string_literal(format!("Feld {SECRET}")),
        }),
    );
    d.doc.objects.insert(
        b,
        Object::Dictionary(dictionary! {
            "Parent" => Object::Reference(a),
            "Kids" => vec![Object::Reference(a)],
            "V" => Object::string_literal(format!("Wert {SECRET}")),
        }),
    );
    mit_annots(&mut d, a);
    let bytes = d.finish();
    let (out, _) = strip_mit_frist(&bytes, "/Parent ↔ /Kids");
    kein_leck(&out, "/Parent ↔ /Kids");
}

/// Zehn Ebenen `/Kids`, jede mit 1 000 Verweisen auf die nächste: ohne
/// Besuchsmenge wären das 10^30 Wege. Der Klartext liegt im Blatt.
#[test]
fn zehn_ebenen_mit_je_tausend_kids_enden() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let ids: Vec<ObjectId> = (0..11).map(|_| d.doc.new_object_id()).collect();
    for ebene in 0..10 {
        let kids = vec![Object::Reference(ids[ebene + 1]); 1000];
        d.doc.objects.insert(
            ids[ebene],
            Object::Dictionary(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
                "Kids" => Object::Array(kids),
            }),
        );
    }
    d.doc.objects.insert(
        ids[10],
        Object::Dictionary(dictionary! {
            "V" => Object::string_literal(format!("Blatt {SECRET}")),
        }),
    );
    mit_annots(&mut d, ids[0]);
    let bytes = d.finish();
    let (out, dauer) = strip_mit_frist(&bytes, "/Kids-Fächer");
    kein_leck(&out, "/Kids-Fächer");
    assert!(dauer < Duration::from_secs(5), "Fächer: {dauer:?}");
}

/// Eine `/Parent`-Kette aus 100 000 Feldern; der Wert hängt am letzten.
/// Ohne Tiefengrenze muss er fallen — mit der alten Grenze von 32 blieb er.
#[test]
fn eine_kette_aus_hunderttausend_feldern_endet_und_faellt() {
    const N: usize = 100_000;
    let mut d: Doc = page(&["Rechnung 4711"]);
    let ids: Vec<ObjectId> = (0..N).map(|_| d.doc.new_object_id()).collect();
    d.doc.objects.insert(
        ids[0],
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Parent" => Object::Reference(ids[1]),
        }),
    );
    for i in 1..N - 1 {
        d.doc.objects.insert(
            ids[i],
            Object::Dictionary(dictionary! { "Parent" => Object::Reference(ids[i + 1]) }),
        );
    }
    d.doc.objects.insert(
        ids[N - 1],
        Object::Dictionary(dictionary! {
            "V" => Object::string_literal(format!("Wert {SECRET}")),
        }),
    );
    mit_annots(&mut d, ids[0]);
    let bytes = d.finish();
    // Die Probe wird nicht durch `load_from_bytes` geschickt (das Budget
    // `max_parsed_bytes` lehnt sie ab — genau so soll es sein, siehe
    // Bericht); gemessen wird der Lauf am Dokument im Speicher.
    let start = Instant::now();
    let mut doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let report = strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let dauer = start.elapsed();
    assert!(dauer < FRIST, "100 000er Kette: {dauer:?}");
    assert_eq!(report.field_values_cleared, 1, "der Wert am Kettenende");
    kein_leck(&out, "100 000er Kette");
}

/// Ein **direkt** eingebettetes Dictionary, so tief verschachtelt, wie der
/// Lader es überhaupt zulässt: `clean_embedded` läuft mit eigenem Stapel über
/// direkte Bäume und darf dort nicht überlaufen.
#[test]
fn ein_tief_eingebetteter_direkter_baum_wird_bereinigt() {
    // 24 Ebenen × 2 (`<<` und `[`) plus Rahmen bleiben unter der
    // Ladegrenze von 100.
    let mut inner = Object::Dictionary(dictionary! {
        "V" => Object::string_literal(format!("Wert {SECRET}")),
    });
    for _ in 0..24 {
        inner = Object::Dictionary(dictionary! { "Kids" => Object::Array(vec![inner]) });
    }
    let mut d: Doc = page(&["Rechnung 4711"]);
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Kids" => Object::Array(vec![inner]),
    }));
    mit_annots(&mut d, a);
    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "Probe trägt das Geheimnis"
    );
    let (out, _) = strip_mit_frist(&bytes, "tiefer direkter Baum");
    kein_leck(&out, "tiefer direkter Baum");
}

/// Und die andere Richtung: was tiefer verschachtelt ist, als der Parser
/// liest, wird beim **Laden** mit Meldung abgelehnt — nicht stillschweigend
/// halb bereinigt. (Das ist die Decke, die den Lauf ohne Tiefengrenze
/// bezahlbar hält.)
#[test]
fn jenseits_der_ladegrenze_wird_abgelehnt_statt_halb_bereinigt() {
    let mut inner = Object::Dictionary(dictionary! {
        "V" => Object::string_literal(format!("Wert {SECRET}")),
    });
    for _ in 0..80 {
        inner = Object::Dictionary(dictionary! { "Kids" => Object::Array(vec![inner]) });
    }
    let mut d: Doc = page(&["Rechnung 4711"]);
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Kids" => Object::Array(vec![inner]),
    }));
    mit_annots(&mut d, a);
    let bytes = d.finish();
    let err = load_from_bytes(&bytes).expect_err("muss abgelehnt werden");
    assert!(
        err.to_string().contains("Verschachtelungstiefe"),
        "die Ablehnung muss die Tiefe nennen: {err}"
    );
}

// ---------------------------------------------------------------------------
// Messung (kein Prüfstück — `#[ignore]`)
// ---------------------------------------------------------------------------

/// Nachgemessen, was der Modulkommentar von `meta.rs` behauptet: „100 000
/// Felder in 0,40 s, eine Kette aus 1 000 000 Feldern in 1,65 s (Release,
/// +40 MiB über dem Dokument)“.
///
/// `cargo test --release -p redact-pdf --test zf_q1_unbegrenzt -- --ignored
/// --nocapture mess_`
#[test]
#[ignore = "Messung, kein Prüfstück"]
fn mess_kette_zeit_und_speicher() {
    fn hwm_mib() -> f64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmHWM:"))
                    .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
            })
            .map(|kib| kib / 1024.0)
            .unwrap_or(f64::NAN)
    }
    for n in [100_000usize, 1_000_000] {
        let mut d: Doc = page(&["Rechnung 4711"]);
        let ids: Vec<ObjectId> = (0..n).map(|_| d.doc.new_object_id()).collect();
        d.doc.objects.insert(
            ids[0],
            Object::Dictionary(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "Parent" => Object::Reference(ids[1]),
            }),
        );
        for i in 1..n - 1 {
            d.doc.objects.insert(
                ids[i],
                Object::Dictionary(dictionary! {
                    "Parent" => Object::Reference(ids[i + 1]),
                    "V" => Object::string_literal("x"),
                }),
            );
        }
        d.doc.objects.insert(
            ids[n - 1],
            Object::Dictionary(dictionary! { "V" => Object::string_literal(SECRET) }),
        );
        mit_annots(&mut d, ids[0]);
        let vor = hwm_mib();
        let start = Instant::now();
        let report = strip_metadata(&mut d.doc);
        let dauer = start.elapsed();
        println!(
            "{n} Felder: strip_metadata {dauer:?}, VmHWM {vor:.0} → {:.0} MiB, \
             field_values_cleared {}",
            hwm_mib(),
            report.field_values_cleared
        );
    }
}
