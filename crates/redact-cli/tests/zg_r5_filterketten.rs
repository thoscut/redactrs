//! Gegenprüfung R5 der Fix-Runde 6: **die sechs Filterketten, über die
//! `SECURITY.md` etwas zusagt — alle sechs an einem Lauf des Binaries.**
//!
//! Die Fix-Runde 6 hat die Zusage verschärft: ein unbekannter Filtername steht
//! „**an jeder Stelle der Kette**, auch als erstes Glied“ in der
//! `NICHT GEPRÜFT`-Liste, und ein **Bildfilter** bleibt „**gleich, an welcher
//! Stelle der Filterkette er steht**“ der benannte blinde Fleck. Unter beiden
//! Sätzen steht in `SECURITY.md` eine Tabelle mit **sechs** Zeilen.
//!
//! Der dort genannte Beleg
//! (`zf_q5_unbekannter_filter::die_zusage_ueber_unbekannte_filter_gilt_an_jeder_stelle_der_kette`)
//! fuhr aber nur **fünf** Ketten: die sechste Zeile,
//! `[/DCTDecode /ASCII85Decode]` — der Bildfilter **am Anfang** —, war an
//! keinen Lauf gebunden. Und genau dort stimmte die Zusage nicht: hinter dem
//! Bildfilter geht die Kette weiter, gelesen hat das Orakel dort nichts, und
//! trotzdem schwieg es. Seit der Fix-Runde 7 ist der blinde Fleck auf das
//! **letzte** Glied beschränkt.
//!
//! Dieser Test bindet alle sechs Zeilen: er baut jede Kette, fährt sie durch
//! das gebaute Binary und verlangt, dass Meldung und Rückgabewert genau so
//! herauskommen, wie die Tabelle sie abdruckt — und dass die Tabelle sie noch
//! genau so abdruckt.
//!
//! Mutationsnachweis: in `crates/redact-pdf/src/audit_bytes.rs` in
//! `decode_stream` das `let stopped = (applied < total).then(…)` auf
//! `(applied < total && applied > 0).then(…)` gesetzt — die alte Fassung, bei
//! der die Meldung an der Position hing. Dann fallen Kette 1 und 2 auf
//! „nicht gefunden“ mit Rückgabewert 0 zurück und dieser Test ist rot.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-zg-r5-{}-{name}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startbar")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Zeilenumbrüche glätten — die Tabelle steht in einer Markdown-Zeile, der
/// Rest der Datei nicht.
fn glatt(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Eine PDF-Datei aus fertigen Objektrümpfen, mit Querverweistabelle.
fn assemble(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (id, body) in objects {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let max = objects.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    out.extend_from_slice(format!("xref\n0 {max}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..max {
        match offsets.iter().find(|(o, _)| *o == id) {
            Some((_, at)) => out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {max} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Der Klartext, verschleiert (XOR 0x5A): weder die Rohbyte- noch eine
/// Zeichenkettensicht findet ihn. Nur wer den Strom auspackt, sieht ihn.
fn verschleiert(text: &str) -> Vec<u8> {
    text.bytes().map(|b| b ^ 0x5A).collect()
}

/// Ein gültiger `FlateDecode`-Strom ohne Bibliothek: unkomprimierte
/// Deflate-Blöcke (`BTYPE = 00`) in einem zlib-Rahmen.
fn flate_stored(daten: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    for (i, block) in daten.chunks(65_535).enumerate() {
        let letzter = (i + 1) * 65_535 >= daten.len();
        out.push(u8::from(letzter));
        let len = u16::try_from(block.len()).expect("Block <= 65 535");
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in daten {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// Eine Datei mit einem Seiteninhalt unter `filter`.
fn pdf_mit_stromfilter(filter: &str, daten: &[u8]) -> Vec<u8> {
    let mut stream =
        format!("<< /Length {} /Filter {filter} >>\nstream\n", daten.len()).into_bytes();
    stream.extend_from_slice(daten);
    stream.extend_from_slice(b"\nendstream");
    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
              /Resources << >> >>"
                .to_vec(),
        ),
        (4, stream),
    ])
}

/// Eine Zeile der Tabelle aus `SECURITY.md`: Kette, Stück der Meldung (leer =
/// „keine“) und Rückgabewert.
struct Zeile {
    kette: &'static str,
    /// Wird der Strom **gepackt** abgelegt (die Kette fängt mit `/FlateDecode`
    /// an) oder roh?
    gepackt: bool,
    meldung: &'static str,
    /// Der Text, den die Tabelle in `SECURITY.md` in ihrer Meldungsspalte
    /// abdruckt — wörtlich, damit die Tabelle nicht unbemerkt wegläuft.
    tabelle: &'static str,
    rc: i32,
}

const TABELLE: [Zeile; 6] = [
    Zeile {
        kette: "/FooDecode",
        gepackt: false,
        meldung: "gar nicht dekodiert — /FooDecode ist hier kein bekannter Filter (Glied 1 von 1)",
        tabelle: "| `/FooDecode` | `gar nicht dekodiert — /FooDecode ist hier kein bekannter \
                  Filter (Glied 1 von 1)` | 3 |",
        rc: 3,
    },
    Zeile {
        kette: "[/FooDecode /FlateDecode]",
        gepackt: false,
        meldung: "(Glied 1 von 2)",
        tabelle: "| `[/FooDecode /FlateDecode]` | `… (Glied 1 von 2)` | 3 |",
        rc: 3,
    },
    Zeile {
        kette: "[/FlateDecode /FooDecode]",
        gepackt: true,
        meldung: "nur bis Filter 1 von 2 dekodiert — /FooDecode ist hier kein bekannter Filter",
        tabelle: "| `[/FlateDecode /FooDecode]` | `nur bis Filter 1 von 2 dekodiert — \
                  /FooDecode ist hier kein bekannter Filter` | 3 |",
        rc: 3,
    },
    Zeile {
        kette: "/DCTDecode",
        gepackt: false,
        meldung: "",
        tabelle: "| `/DCTDecode` | keine | 0 |",
        rc: 0,
    },
    Zeile {
        kette: "[/FlateDecode /DCTDecode]",
        gepackt: true,
        meldung: "",
        tabelle: "| `[/FlateDecode /DCTDecode]` | keine | 0 |",
        rc: 0,
    },
    // Die sechste Zeile: der Bildfilter **am Anfang** der Kette. Sie war der
    // einzige Fall, den der in `SECURITY.md` genannte Beleg nicht fuhr — und
    // genau dort stimmte die Zusage nicht: bis zur Fix-Runde 7 schwieg das
    // Orakel auch hier, obwohl es hinter dem Bildfilter nichts gelesen hatte.
    // Seit der Fix-Runde 7 ist der blinde Fleck auf das **letzte** Glied
    // beschränkt; geht die Kette dahinter weiter, steht die Stelle in der
    // `NICHT GEPRÜFT`-Liste.
    Zeile {
        kette: "[/DCTDecode /ASCII85Decode]",
        gepackt: false,
        meldung: "/DCTDecode ist ein Bildfilter und wird nicht dekodiert, aber die Kette geht dahinter weiter (Glied 1 von 2)",
        tabelle: "| `[/DCTDecode /ASCII85Decode]` | `gar nicht dekodiert — /DCTDecode ist \
                  ein Bildfilter und wird nicht dekodiert, aber die Kette geht dahinter \
                  weiter (Glied 1 von 2)` | 3 |",
        rc: 3,
    },
];

/// **Alle sechs Zeilen der Tabelle, jede an einem Lauf.**
#[test]
fn jede_zeile_der_filterkettentabelle_stammt_aus_einem_lauf() {
    let dir = workdir("ketten");
    let geheim = "GEHEIM Max Mustermann";
    let roh = verschleiert(geheim);
    let gepackt = flate_stored(&roh);

    for (nr, zeile) in TABELLE.iter().enumerate() {
        let datei = format!("k{nr}.pdf");
        let daten: &[u8] = if zeile.gepackt { &gepackt } else { &roh };
        std::fs::write(dir.join(&datei), pdf_mit_stromfilter(zeile.kette, daten)).unwrap();

        let out = run_in(&dir, &[&datei, "--check-leaks", "GEHEIM"]);
        let text = stdout(&out);
        assert_eq!(
            out.status.code(),
            Some(zeile.rc),
            "{}: Rückgabewert {:?} statt {}:\n{text}",
            zeile.kette,
            out.status.code(),
            zeile.rc
        );
        if zeile.meldung.is_empty() {
            assert!(
                !text.contains("  NICHT GEPRÜFT: "),
                "{}: eine Meldung, obwohl die Tabelle „keine“ sagt — jede Datei mit \
                 einem Foto käme so als unvollständig geprüft zurück:\n{text}",
                zeile.kette
            );
        } else {
            assert!(
                text.contains(zeile.meldung),
                "{}: die Tabelle druckt „{}“, der Lauf sagt das nicht:\n{text}",
                zeile.kette,
                zeile.meldung
            );
        }
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Und die Tabelle in `SECURITY.md` trägt genau diese sechs Zeilen — sonst
/// bindet der Lauf oben etwas, das dort gar nicht mehr steht.
#[test]
fn die_tabelle_in_security_md_druckt_genau_diese_sechs_ketten() {
    let security = glatt(
        &std::fs::read_to_string(repo_root().join("SECURITY.md"))
            .expect("SECURITY.md lesbar")
            .replace("\r\n", "\n"),
    );
    for zeile in &TABELLE {
        let erwartet = glatt(zeile.tabelle);
        assert!(
            security.contains(&erwartet),
            "SECURITY.md druckt die Zeile nicht mehr: {erwartet}"
        );
    }
    let ohne_meldung = TABELLE.iter().filter(|z| z.meldung.is_empty()).count();
    assert_eq!(
        ohne_meldung, 2,
        "nur ein Bildfilter **am Kettenende** ist ein stiller blinder Fleck"
    );
    assert_eq!(
        security.matches("| keine | 0 |").count(),
        ohne_meldung,
        "die Tabelle nennt nicht mehr genau {ohne_meldung} Ketten ohne Meldung"
    );

    // Und der Satz **über** der Tabelle nennt ihre Zahl. Bis zur Fix-Runde 7
    // stand dort „fünf Ketten“ über sechs Zeilen, und der genannte Beleg fuhr
    // nur fünf davon.
    let wort = match TABELLE.len() {
        5 => "fünf",
        6 => "sechs",
        7 => "sieben",
        n => panic!("für {n} Ketten gibt es hier kein Zahlwort"),
    };
    assert!(
        security.contains(&format!("Am gebauten Binary nachgemessen, {wort} Ketten")),
        "SECURITY.md nennt über der Tabelle nicht {wort} Ketten"
    );
}

/// **Die Gegenrichtung.** Eine gewöhnliche Datei — Klartext unter
/// `/FlateDecode` — wird gefunden, und eine ohne den Begriff endet sauber mit
/// 0 und **ohne** `NICHT GEPRÜFT`. Eine Regel, die harmlose Dateien anmahnt,
/// wäre genauso ein Fehler wie eine Lücke.
#[test]
fn eine_gewoehnliche_datei_loest_keinen_falschen_alarm_aus() {
    let dir = workdir("harmlos");
    let klartext = b"BT /F1 10 Tf 1 0 0 1 40 700 Tm (GEHEIM Max Mustermann) Tj ET";
    std::fs::write(
        dir.join("klar.pdf"),
        pdf_mit_stromfilter("/FlateDecode", &flate_stored(klartext)),
    )
    .unwrap();

    let gefunden = run_in(&dir, &["klar.pdf", "--check-leaks", "GEHEIM"]);
    assert_eq!(
        gefunden.status.code(),
        Some(3),
        "der Klartext steht drin und wird nicht gemeldet:\n{}",
        stdout(&gefunden)
    );

    let sauber = run_in(&dir, &["klar.pdf", "--check-leaks", "STEHTNICHTDRIN"]);
    let text = stdout(&sauber);
    assert!(
        !text.contains("NICHT GEPRÜFT"),
        "falscher Alarm an einer gewöhnlichen Datei:\n{text}"
    );
    assert_eq!(
        sauber.status.code(),
        Some(0),
        "falscher Rückgabewert an einer gewöhnlichen Datei:\n{text}"
    );

    std::fs::remove_dir_all(&dir).ok();
}
