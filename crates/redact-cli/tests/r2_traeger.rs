//! Prüfrunde 2, Träger: Stellen, an denen ein Geheimnis die Schwärzung
//! überlebte, während der Lauf mit 0 endete — und die jetzt fallen.
//!
//! Jede Probe: eine Seite mit sichtbarer IBAN (die das Vorgabemuster findet
//! und schwärzt) und **ein** zusätzlicher Träger mit derselben IBAN. Gemessen
//! wird am Binary: Rückgabewert der Schwärzung, dann `--check-leaks` an der
//! Ausgabe, dazu eine unabhängige Suche über alle entpackten Objekte. Ein
//! Lauf mit Rückgabewert 0, nach dem eine der beiden Prüfungen die IBAN
//! findet, ist ein stilles Leck.
//!
//! Die Gegenmaßnahmen stehen in `redact_pdf::meta`: die Erlaubnislisten unter
//! den erlaubten Schlüsseln (`Shape`), die Erlaubnislisten der Annotationen
//! je Untertyp (`carrier_allows`), der Trägerlauf über jede erreichbare
//! Annotation (`clean_unreached_carriers`) und die Type3-Schriften, die
//! niemand setzt (`clear_unset_type3_fonts`).

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("redact-r2-traeger-{}-{name}", std::process::id(),));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startet")
}

struct Base {
    doc: Document,
    catalog: ObjectId,
    page: ObjectId,
    resources: ObjectId,
    font: ObjectId,
}

fn base() -> Base {
    let mut doc = Document::with_version("1.7");
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let content = format!("BT /F1 10 Tf 72 700 Td (IBAN: {SECRET}) Tj ET\n");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content_id,
        "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1_i64,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    Base {
        doc,
        catalog,
        page,
        resources,
        font,
    }
}

fn s(text: &str) -> Object {
    Object::string_literal(text)
}

fn dict_mut(doc: &mut Document, id: ObjectId) -> &mut Dictionary {
    doc.get_dictionary_mut(id).unwrap()
}

/// Ergebnis einer Probe.
struct Probe {
    name: &'static str,
    redact_code: i32,
    check_code: i32,
    /// Unabhängige Suche: steht die IBAN (oder nur ihre Ziffernfolge) noch
    /// irgendwo in der Ausgabe — roh, in einem entpackten Strom, als Name?
    independent: Vec<String>,
}

fn independent_search(bytes: &[u8]) -> Vec<String> {
    let squeezed: String = SECRET.chars().filter(|c| !c.is_whitespace()).collect();
    let digits = &squeezed[4..]; // "370400440532013000"
    let mut hay: Vec<(String, Vec<u8>)> = vec![("roh".into(), bytes.to_vec())];
    if let Ok(doc) = Document::load_mem(bytes) {
        for (id, obj) in &doc.objects {
            if let Object::Stream(st) = obj {
                let data = st
                    .decompressed_content()
                    .unwrap_or_else(|_| st.content.clone());
                hay.push((format!("Strom {id:?}"), data));
            }
            hay.push((format!("Objekt {id:?}"), format!("{obj:?}").into_bytes()));
        }
    }
    let mut out = Vec::new();
    for (site, h) in hay {
        let text = String::from_utf8_lossy(&h);
        let plain: String = text
            .replace("%20", "")
            .replace("#20", "")
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect();
        if plain.contains(digits) {
            out.push(site);
        }
    }
    out
}

fn probe(name: &'static str, build: impl FnOnce(&mut Base)) -> Probe {
    let mut b = base();
    build(&mut b);
    let dir = workdir(name);
    let input = dir.join("in.pdf");
    let output = dir.join("out.pdf");
    b.doc.save(&input).unwrap();
    let r = run(&[input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    let redact_code = r.status.code().unwrap_or(-1);
    if !output.exists() {
        eprintln!(
            "{name}: keine Ausgabe, code {redact_code}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        return Probe {
            name,
            redact_code,
            check_code: -1,
            independent: vec![],
        };
    }
    let c = run(&[output.to_str().unwrap(), "--check-leaks", SECRET]);
    let check_code = c.status.code().unwrap_or(-1);
    let independent = independent_search(&std::fs::read(&output).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    Probe {
        name,
        redact_code,
        check_code,
        independent,
    }
}

/// Gate: ein Lauf mit Rückgabewert 0, nach dem `--check-leaks` die IBAN
/// findet (oder die unabhängige Suche sie findet), ist ein stilles Leck.
#[track_caller]
fn assert_no_silent_leak(probes: &[Probe]) {
    let mut bad = Vec::new();
    for p in probes {
        let leaked = p.check_code == 3 || !p.independent.is_empty();
        if p.redact_code == 0 && leaked {
            bad.push(format!(
                "{}: Schwärzung 0, check-leaks {}, unabhängig {:?}",
                p.name, p.check_code, p.independent
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "STILLE LECKS (Rückgabewert 0, Geheimnis in der Ausgabe):\n  {}",
        bad.join("\n  ")
    );
}

/// Die Erlaubnislisten von Katalog und Seite wirkten nur eine Ebene tief:
/// was **unter** einem erlaubten Schlüssel stand (Katalog
/// `/ViewerPreferences`, `/Extensions`, `/PageLabels`-Eintrag; Seite
/// `/Group`, `/Trans`, `/BoxColorInfo`), blieb mit jedem fremden Schlüssel
/// stehen. Jetzt gilt dort die Schlüsselliste der Norm.
#[test]
fn erlaubte_schluessel_tragen_fremde_unterschluessel() {
    let probes = vec![
        probe("viewerprefs_fremd", |b| {
            dict_mut(&mut b.doc, b.catalog).set(
                "ViewerPreferences",
                dictionary! { "DisplayDocTitle" => true, "Zusatz" => s(SECRET) },
            );
        }),
        probe("extensions_url", |b| {
            dict_mut(&mut b.doc, b.catalog).set(
                "Extensions",
                dictionary! { "ADBE" => dictionary! {
                    "BaseVersion" => "1.7", "ExtensionLevel" => 3,
                    "URL" => s(&format!("https://x.example/{SECRET}")),
                }},
            );
        }),
        probe("pagelabel_fremd", |b| {
            dict_mut(&mut b.doc, b.catalog).set(
                "PageLabels",
                dictionary! { "Nums" => vec![0.into(), Object::Dictionary(dictionary! {
                "S" => "D", "Zusatz" => s(SECRET) })] },
            );
        }),
        probe("page_group_fremd", |b| {
            dict_mut(&mut b.doc, b.page).set(
                "Group",
                dictionary! { "S" => "Transparency", "CS" => "DeviceRGB", "Zusatz" => s(SECRET) },
            );
        }),
        probe("page_trans_fremd", |b| {
            dict_mut(&mut b.doc, b.page)
                .set("Trans", dictionary! { "S" => "R", "Zusatz" => s(SECRET) });
        }),
        probe("page_boxcolorinfo", |b| {
            dict_mut(&mut b.doc, b.page).set(
                "BoxColorInfo",
                dictionary! { "CropBox" => dictionary! { "W" => 1, "Zusatz" => s(SECRET) } },
            );
        }),
    ];
    assert_no_silent_leak(&probes);
}

/// Die Einträge **unter** den Ressourcenarten — Schrift, Schriftbeschreibung
/// (`/Style /Panose`, `/CharSet`), ExtGState, Shading, Bild-XObject, `/OCG` —
/// behielten jeden fremden Schlüssel, und die Glyphprozedur einer Type3-
/// Schrift, die niemand setzt, ging durch keine Schwärzung.
#[test]
fn ressourcen_eintraege_tragen_fremde_schluessel() {
    let probes = vec![
        probe("font_fremd", |b| {
            dict_mut(&mut b.doc, b.font).set("Zusatz", s(SECRET));
        }),
        probe("font_descriptor_style_charset", |b| {
            let fd = b.doc.add_object(dictionary! {
                "Type" => "FontDescriptor", "FontName" => "Helvetica", "Flags" => 32,
                "Style" => dictionary! { "Panose" => s(SECRET) },
                "CharSet" => s(SECRET),
            });
            dict_mut(&mut b.doc, b.font).set("FontDescriptor", fd);
        }),
        probe("extgstate_fremd", |b| {
            let gs = b
                .doc
                .add_object(dictionary! { "Type" => "ExtGState", "Zusatz" => s(SECRET) });
            dict_mut(&mut b.doc, b.resources).set("ExtGState", dictionary! { "GS0" => gs });
        }),
        probe("shading_fremd", |b| {
            let sh = b.doc.add_object(dictionary! {
                "ShadingType" => 2, "ColorSpace" => "DeviceGray",
                "Coords" => vec![0.into(),0.into(),1.into(),0.into()],
                "Function" => dictionary! { "FunctionType" => 2, "Domain" => vec![0.into(),1.into()], "N" => 1 },
                "Zusatz" => s(SECRET),
            });
            dict_mut(&mut b.doc, b.resources).set("Shading", dictionary! { "Sh0" => sh });
        }),
        probe("ocg_fremd", |b| {
            let ocg = b.doc.add_object(dictionary! {
                "Type" => "OCG", "Name" => s("Ebene"), "Zusatz" => s(SECRET),
            });
            dict_mut(&mut b.doc, b.resources).set("Properties", dictionary! { "OC0" => ocg });
        }),
        probe("image_fremd", |b| {
            let img = b.doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
                    "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                    "Zusatz" => s(SECRET),
                },
                vec![0u8],
            ));
            dict_mut(&mut b.doc, b.resources).set("XObject", dictionary! { "Im0" => img });
            let content = format!(
                "BT /F1 10 Tf 72 700 Td (IBAN: {SECRET}) Tj ET q 10 0 0 10 400 100 cm /Im0 Do Q\n"
            );
            let cid = b
                .doc
                .add_object(Stream::new(dictionary! {}, content.into_bytes()));
            dict_mut(&mut b.doc, b.page).set("Contents", cid);
        }),
        // Eine Type3-Schrift, deren **unbenutzte** Glyphe den Text zeichnet:
        // `/CharProcs` geht durch keine Schwärzung.
        probe("type3_unbenutzte_glyphe", |b| {
            let proc_ = b.doc.add_object(Stream::new(
                dictionary! {},
                format!("0 0 d0 BT /F1 1 Tf ({SECRET}) Tj ET").into_bytes(),
            ));
            let t3 = b.doc.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "Type3",
                "FontBBox" => vec![0.into(),0.into(),1.into(),1.into()],
                "FontMatrix" => vec![0.001.into(),0.into(),0.into(),0.001.into(),0.into(),0.into()],
                "CharProcs" => dictionary! { "a" => proc_ },
                "Encoding" => dictionary! { "Differences" => vec![97.into(), "a".into()] },
                "FirstChar" => 97, "LastChar" => 97, "Widths" => vec![1000.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => b.font } },
            });
            let r = dict_mut(&mut b.doc, b.resources);
            if let Ok(Object::Dictionary(f)) = r.get_mut(b"Font") {
                f.set("T3", t3);
            }
        }),
    ];
    assert_no_silent_leak(&probes);
}

/// Annotationen liefen nur über eine **Sperrliste** (`clean_carrier`): ein
/// fremder Schlüssel an einer Annotation fern jeder Schwärzung blieb; und
/// eine Annotation, die nicht in `/Annots` hängt (hier: unter
/// `/Properties`), erreichte der Trägerlauf gar nicht — samt `/Contents` und
/// `/A /URI`.
#[test]
fn annotationen_sperrliste_und_unerreichte_annotation() {
    let probes = vec![
        probe("annot_fremd", |b| {
            let a = b.doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Square",
                "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
                "Zusatz" => s(SECRET),
            });
            dict_mut(&mut b.doc, b.page).set("Annots", vec![Object::Reference(a)]);
        }),
        probe("annot_nur_ueber_properties", |b| {
            let a = b.doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Link",
                "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
                "A" => dictionary! { "S" => "URI", "URI" => s("https://bank.example/?iban=DE89370400440532013000") },
                "Contents" => s(SECRET),
            });
            dict_mut(&mut b.doc, b.resources).set(
                "Properties",
                dictionary! { "MC0" => dictionary! { "Annot" => a } },
            );
        }),
    ];
    assert_no_silent_leak(&probes);
}

/// Gegenprobe: Stellen, die der Lauf schon vorher zusagte, sind sauber —
/// mit demselben Messgerät, damit ein Fehlalarm der Proben auffiele.
#[test]
fn gegenprobe_zugesagte_stellen() {
    let probes = vec![
        probe("trailer_id", |b| {
            b.doc.trailer.set("ID", vec![s(SECRET), s(SECRET)]);
        }),
        probe("metadata_am_font", |b| {
            let xmp = b.doc.add_object(Stream::new(
                dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
                format!("<x:xmpmeta>{SECRET}</x:xmpmeta>").into_bytes(),
            ));
            dict_mut(&mut b.doc, b.font).set("Metadata", xmp);
        }),
        probe("link_uri", |b| {
            let a = b.doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Link",
                "Rect" => vec![400.into(), 100.into(), 420.into(), 120.into()],
                "A" => dictionary! { "S" => "URI", "URI" => s("https://bank.example/?iban=DE89%203704%200044%200532%200130%2000") },
            });
            dict_mut(&mut b.doc, b.page).set("Annots", vec![Object::Reference(a)]);
        }),
    ];
    assert_no_silent_leak(&probes);
}
