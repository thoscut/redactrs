//! Gegenprüfung P4 der Fix-Runde 4: die Grenzen von `--check-leaks` am
//! gebauten Binary.
//!
//! Zwei Fragen, beide an der Kommandozeile gemessen:
//!
//! 1. Hält der neue Rückgabewert 3 („nicht geprüft“) auf **jedem** Weg —
//!    auch unter `--quiet`, auch mit Begriffen aus der Standardeingabe, auch
//!    zusammen mit einem Fund? Und bleibt `--json` weiterhin abgelehnt, damit
//!    es keine zweite, stillere Antwort gibt? Das prüft
//!    [`nicht_geprueft_gilt_auf_jedem_weg`] (grün).
//!
//! 2. Gibt es eine Datei, die die Vorprüfung passiert, in der die Suche eine
//!    Stelle **auslässt** und die trotzdem mit 0 endet? Ja: die Objektsicht
//!    des Orakels bricht bei Verschachtelungstiefe 32 ab
//!    (`redact_pdf::audit_bytes::MAX_DEPTH`), der Lader lässt 100 zu. Ein
//!    oktal maskierter Text auf Ebene 33 wird von keiner Sicht gelesen und
//!    trotzdem als „nicht gefunden“ mit Rückgabewert 0 gemeldet — die stille
//!    Entwarnung, die diese Runde abstellen wollte. Der Nachweis steht in
//!    [`tiefe_33_ist_eine_stille_entwarnung`]. Seit der Fix-Runde 5 meldet die
//!    Objektsicht ihren Abbruch (`NICHT GEPRÜFT: … Verschachtelungstiefe 32
//!    erreicht`, Rückgabewert 3); der Test ist deshalb **scharf** und läuft im
//!    Gate mit: `cargo test -p redact-cli --test ze_p4_check_leaks_grenzen`

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-ze-p4-{}-{name}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_in(dir: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args(args)
        .current_dir(dir)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD");
    match stdin {
        None => {
            cmd.stdin(Stdio::null());
            cmd.output().expect("Binary startbar")
        }
        Some(text) => {
            use std::io::Write;
            let mut kind = cmd
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Binary startbar");
            kind.stdin
                .as_mut()
                .expect("stdin")
                .write_all(text.as_bytes())
                .expect("stdin schreibbar");
            kind.wait_with_output().expect("Lauf endet")
        }
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Eine PDF-Datei aus fertigen Objektrümpfen, mit Querverweistabelle.
fn assemble(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push(out.len());
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Ein Strom **ohne `/Filter`**, dessen Bytes zlib sind: die Vorprüfung zählt
/// ihn roh (wenige Kilobyte) und lässt die Datei durch, die Rohsicht der
/// Nachprüfung bläst ihn auf. Genau die Stelle, an der das Budget greifen und
/// der Lauf „nicht geprüft“ sagen muss. `/Titel` trägt den Fund, damit sich
/// beide Sätze zusammen zeigen lassen.
fn pdf_mit_getarntem_flate_strom(megabytes: usize, geheim: &str) -> Vec<u8> {
    use lopdf::{dictionary, Stream};

    let mut stream = Stream::new(dictionary! {}, vec![0u8; megabytes * 1024 * 1024]);
    stream.compress().expect("komprimierbar");
    let mut blob = format!("<< /Length {} >>\nstream\n", stream.content.len()).into_bytes();
    blob.extend_from_slice(&stream.content);
    blob.extend_from_slice(b"\nendstream");

    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
                 /Titel ({geheim}) >>"
            )
            .into_bytes(),
        ),
        (4, b"<< /Length 5 >>\nstream\nBT ET\nendstream".to_vec()),
        (5, blob),
    ])
}

/// Ein `RunLengthDecode`-Strom, der sich weit über jedes kleine Budget
/// aufbläst — und den die **Vorprüfung** des Laders nicht auspackt (sie
/// dekodiert nur Flate, LZW und ASCII85 und zählt hier die Rohbytes). Genau
/// die Lücke, in der die Objektsicht der Nachprüfung einen Strom auslässt,
/// ohne dass der Lauf schon vorher mit Rückgabewert 1 endet — und damit die
/// einzige Art Datei, an der die Kommandozeile den Grund „Sicht 7
/// (Schriftdekoder) nicht gelaufen“ zeigen kann.
fn pdf_mit_runlength_bombe(wiederholungen: usize) -> Vec<u8> {
    // 129 heißt „das nächste Byte 128-mal“ — zwei Byte Eingabe, 128 Byte
    // Ausgabe. 0x80 beendet den Strom.
    let mut daten = Vec::with_capacity(wiederholungen * 2 + 1);
    for _ in 0..wiederholungen {
        daten.push(129u8);
        daten.push(b'A');
    }
    daten.push(0x80);
    let mut blob = format!(
        "<< /Length {} /Filter /RunLengthDecode >>\nstream\n",
        daten.len()
    )
    .into_bytes();
    blob.extend_from_slice(&daten);
    blob.extend_from_slice(b"\nendstream");
    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, blob),
    ])
}

/// „nicht geprüft“ ist auf jedem Weg 3 — und `--json` bleibt abgelehnt.
///
/// Vier Wege: laut, `--quiet`, Begriffe aus der Standardeingabe (`-`), und
/// zusammen mit einem Fund. In allen vier steht die Zeile `NICHT GEPRÜFT:`
/// auf **stdout** (auch unter `--quiet`, sonst bliebe unter `> bericht.txt`
/// genau die harmlose Hälfte), und der Rückgabewert ist 3.
///
/// Mutation (nachgewiesen): in `check::report` den Zweig `if unchecked > 0`
/// vor der Entwarnung entfernen — der Lauf endet mit 0, der Test ist rot.
#[test]
fn nicht_geprueft_gilt_auf_jedem_weg() {
    let dir = workdir("wege");
    let geheim = "GEHEIM-IM-TITEL";
    let datei = "getarnt.pdf";
    let bytes = pdf_mit_getarntem_flate_strom(4, geheim);
    assert!(bytes.len() < 64 * 1024, "{} Byte", bytes.len());
    std::fs::write(dir.join(datei), &bytes).unwrap();

    // (a) laut, ohne Fund
    let out = run_in(
        &dir,
        &[
            datei,
            "--check-leaks",
            "NICHT-DRIN",
            "--max-decompressed-mb",
            "1",
        ],
        None,
    );
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "laut: {text}");
    assert!(text.contains("  NICHT GEPRÜFT: "), "laut: {text}");
    assert!(
        !text.contains("steht nicht mehr in der Datei"),
        "Entwarnung trotz nicht geprüfter Stelle: {text}"
    );

    // (b) --quiet: die Zeile bleibt, der Rückgabewert auch
    let out = run_in(
        &dir,
        &[
            datei,
            "--check-leaks",
            "NICHT-DRIN",
            "--max-decompressed-mb",
            "1",
            "--quiet",
        ],
        None,
    );
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "quiet: {text}");
    assert!(text.contains("  NICHT GEPRÜFT: "), "quiet: {text}");
    assert!(
        text.contains("nicht geprüft — die Antwort ist unvollständig"),
        "quiet: {text}"
    );

    // (c) Begriffe von der Standardeingabe
    let out = run_in(
        &dir,
        &[datei, "--check-leaks", "-", "--max-decompressed-mb", "1"],
        Some("NICHT-DRIN\nAUCH-NICHT\n"),
    );
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "stdin: {text}");
    assert!(text.contains("  NICHT GEPRÜFT: "), "stdin: {text}");

    // (d) Fund UND nicht geprüfte Stelle: beide Sätze
    let out = run_in(
        &dir,
        &[datei, "--check-leaks", geheim, "--max-decompressed-mb", "1"],
        None,
    );
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "beides: {text}");
    assert!(text.contains("  GEFUNDEN ("), "beides: {text}");
    assert!(
        text.contains("nicht geprüft — die Antwort ist unvollständig"),
        "beides: der zweite Satz fehlt: {text}"
    );

    // (e) `--json` bleibt ausgeschlossen — keine zweite, stillere Antwort.
    let out = run_in(
        &dir,
        &[datei, "--check-leaks", "NICHT-DRIN", "--json"],
        None,
    );
    assert_eq!(out.status.code(), Some(2), "--json: {}", stdout(&out));

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein Text auf Verschachtelungsebene `depth`, oktal maskiert — damit ihn
/// keine Bytesuche über die Rohdatei findet, sondern nur die Objektsicht,
/// die die Maskierung auflöst.
fn pdf_mit_tiefem_text(depth: usize, geheim: &str) -> Vec<u8> {
    let maskiert: String = geheim.bytes().map(|b| format!("\\{b:03o}")).collect();
    let mut tief = format!("({maskiert})");
    for _ in 0..depth {
        tief = format!("[{tief}]");
    }
    assemble(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R /Tief 5 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, b"<< /Length 5 >>\nstream\nBT ET\nendstream".to_vec()),
        (5, tief.into_bytes()),
    ])
}

/// **Befund P4-2 — geschlossen in der Fix-Runde 5, dieser Test ist scharf.**
///
/// Der Befund: auf Ebene 32 fand die Nachprüfung den Text (Rückgabewert 3),
/// auf Ebene 33 meldete sie „nicht gefunden“ mit Rückgabewert **0** und ohne
/// eine einzige `NICHT GEPRÜFT`-Zeile — obwohl der Text in der Datei steht und
/// keine Sicht ihn gelesen hatte. Die Vorprüfung des Laders lässt 100 Ebenen zu
/// (`Limits::max_nesting_depth`), die Objektsicht des Orakels bricht bei 32 ab
/// (`audit_bytes::MAX_DEPTH`) — und meldete das Abbrechen nicht.
///
/// Verlangt wird, was die Runde 4 zugesagt hat: entweder finden oder benennen,
/// aber nie stillschweigend 0. Der Lauf sagt jetzt beides:
///
/// ```text
/// Tiefe 32:  GEFUNDEN (3 Fundstelle(n)): DE89 3704 0044 0532 0130 00
///            Objekt 5 0[0]…[0] [Zeichenkette, literal]: …      → 3
/// Tiefe 33:  GEFUNDEN (1 Fundstelle(n)): DE89 3704 0044 0532 0130 00
///            Rohdatei @0x… [Zeichenkette (dekodiert)]: …
///            NICHT GEPRÜFT: Objekt 5 0[0]…[0]: nicht durchsucht —
///            Verschachtelungstiefe 32 erreicht; was tiefer liegt, hat
///            keine Sicht gelesen                                 → 3
/// ```
///
/// Die Zeichenkette ist oktal maskiert, damit die Rohsicht der Datei sie
/// nicht als Bytes trifft. Bis zur Spur-A-Runde 1 (Register #80) hieß Tiefe
/// 33 deshalb „nicht gefunden“ plus `NICHT GEPRÜFT`; seither liest ein
/// zweiter Gang der Rohsicht jedes Zeichenketten-Literal dekodiert und
/// findet sie auch dort, wo die Objektsicht nicht mehr hinkommt. Die
/// Objektsicht selbst findet sie auf Ebene 33 weiterhin nicht, und sie sagt
/// es weiterhin — das ist, was dieser Test hält.
#[test]
fn tiefe_33_ist_eine_stille_entwarnung() {
    let dir = workdir("tiefe");
    let geheim = "DE89 3704 0044 0532 0130 00";

    for (tiefe, gefunden) in [(32usize, true), (33, false)] {
        let datei = format!("tief{tiefe}.pdf");
        std::fs::write(dir.join(&datei), pdf_mit_tiefem_text(tiefe, geheim)).unwrap();
        let out = run_in(&dir, &[&datei, "--check-leaks", geheim], None);
        let text = stdout(&out);
        assert_eq!(
            text.contains("[Zeichenkette, literal]"),
            gefunden,
            "Tiefe {tiefe}: {text}"
        );
        assert!(
            text.contains("[Zeichenkette (dekodiert)]") && text.contains("  GEFUNDEN ("),
            "Tiefe {tiefe}: die dekodierte Zeichenkette der Rohsicht fehlt:\n{text}"
        );
        assert_eq!(
            out.status.code(),
            Some(3),
            "Tiefe {tiefe}: stille Entwarnung — der Text steht in der Datei, \
             keine Sicht hat ihn gelesen, und der Lauf sagt es nicht:\n{text}"
        );
        // Ebene 33 liest die Objektsicht nicht — dann muss sie benannt
        // sein, mit Grund. Der Fund der Rohsicht ersetzt das nicht: was
        // keine Sicht gelesen hat, bleibt eine offene Stelle.
        if !gefunden {
            assert!(
                text.contains("  NICHT GEPRÜFT: "),
                "Tiefe {tiefe}: kein Fund und keine benannte Stelle:\n{text}"
            );
            assert!(
                text.contains("Verschachtelungstiefe 32 erreicht"),
                "Tiefe {tiefe}: die Stelle nennt ihren Grund nicht:\n{text}"
            );
            assert!(
                text.contains("nicht geprüft — die Antwort ist unvollständig"),
                "Tiefe {tiefe}: das Ergebnis liest sich wie eine Entwarnung:\n{text}"
            );
            // Und der Rat am Ende schickt niemanden an den falschen Schalter:
            // ein höheres Entpackbudget hilft gegen die Tiefengrenze nicht.
            // Der Rat nennt die Ursachen, gegen die der Schalter nichts
            // ausrichtet, in einem Satzteil „was an … hängt, nicht“ — die
            // Tiefengrenze muss darin stehen; welche Ursachen er sonst noch
            // nennt (seit Fix-Runde 6 auch den unbekannten Filternamen), ist
            // hier nicht die Frage.
            let rat = text.split("hängt, nicht").next().unwrap_or_default();
            assert!(
                rat.contains("was an der Verschachtelungstiefe"),
                "Tiefe {tiefe}: der Satz verspricht --max-decompressed-mb als \
                 Heilmittel für jede Ursache:\n{text}"
            );
        }
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// **Gegenprüfung der Fix-Runde 6, E3: die Gründe der Ausgabe stehen in
/// `SECURITY.md`.**
///
/// `SECURITY.md` zählte drei Gründe für eine `NICHT GEPRÜFT`-Zeile auf, das
/// Orakel kennt fünf. Die Aufzählung ist jetzt vollständig
/// (`belege.rs::die_fuenf_gruende_fuer_nicht_geprueft_stehen_in_security_md`
/// hält sie gegen den Quelltext des Orakels); hier stehen die Gründe, die die
/// **Kommandozeile** wirklich erreichen kann, gegen einen Lauf des gebauten
/// Binaries. Zwei sind es an dieser Stelle:
///
/// * die Entpackgrenze (`--max-decompressed-mb`) — an einem Strom ohne
///   `/Filter`, dessen Bytes zlib sind: die Vorprüfung des Laders zählt ihn
///   roh, die Rohsicht bläst ihn auf und lehnt ihn am Budget ab,
/// * die Verschachtelungstiefe der Objektsicht.
///
/// Der dritte (ein unbekannter Filtername) steht in
/// `zf_q5_unbekannter_filter.rs`. Die beiden übrigen kann die Kommandozeile
/// nicht zeigen: `check::run` lädt die Datei vorher mit denselben Grenzen,
/// und seit der Spur-A-Runde 1 (Register #64) packt die Vorprüfung jede
/// Kette aus, die das Orakel auch auspackt — was in der Objektsicht am
/// Budget scheiterte, lehnt sie vorher ab (Rückgabewert 1, Budget als
/// Grund). Damit erreicht die Kommandozeile weder „Vorprüfung abgelehnt“
/// als `NICHT GEPRÜFT`-Zeile noch den nicht gelaufenen Schriftdekoder (der
/// einem übersprungenen Strom der Objektsicht folgt). Bis #64 zeigte eine
/// RunLength-Bombe beides; die Probe steht unten als (a′) und hält fest,
/// dass der Lader sie jetzt mit dem Budget ablehnt. Beide Gründe gehören
/// der Oberfläche, die das Orakel ohne den Ladeschritt aufruft; ihr Wortlaut
/// bleibt in `SECURITY.md` und wird hier weiter dort verlangt.
///
/// Mutationsnachweis: in `SECURITY.md` die Zeile „Schriftdekoder nicht
/// gelaufen“ aus der Gründetabelle gestrichen → dieser Test ist rot.
#[test]
fn die_gruende_der_ausgabe_stehen_in_security_md() {
    let wurzel = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let security = std::fs::read_to_string(wurzel.join("SECURITY.md"))
        .expect("SECURITY.md lesbar")
        .replace("\r\n", "\n");
    let security = security.split_whitespace().collect::<Vec<_>>().join(" ");

    let dir = workdir("gruende");
    let mut gesehen: Vec<String> = Vec::new();

    // (a) Entpackgrenze — an der Rohsicht, hinter einem Strom ohne /Filter.
    std::fs::write(
        dir.join("getarnt.pdf"),
        pdf_mit_vielen_zu_grossen_stroemen(1, 2),
    )
    .unwrap();
    let out = run_in(
        &dir,
        &[
            "getarnt.pdf",
            "--check-leaks",
            "NICHT-DRIN",
            "--max-decompressed-mb",
            "1",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(3), "{}", stdout(&out));
    gesehen.extend(stdout(&out).lines().map(str::to_string));

    // (a′) Dieselbe Grenze an einer RunLength-Bombe: seit #64 lehnt sie der
    // Lader ab, bevor das Orakel läuft — Rückgabewert 1, Budget als Grund.
    std::fs::write(dir.join("rl.pdf"), pdf_mit_runlength_bombe(20_000)).unwrap();
    let out = run_in(
        &dir,
        &[
            "rl.pdf",
            "--check-leaks",
            "NICHT-DRIN",
            "--max-decompressed-mb",
            "1",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let fehler = String::from_utf8_lossy(&out.stderr);
    assert!(
        fehler.contains("Budget"),
        "der Lader nennt das Budget nicht als Grund: {fehler}"
    );

    // (b) Verschachtelungstiefe.
    std::fs::write(
        dir.join("tief33.pdf"),
        pdf_mit_tiefem_text(33, "DE89 3704 0044 0532 0130 00"),
    )
    .unwrap();
    let out = run_in(
        &dir,
        &["tief33.pdf", "--check-leaks", "DE89 3704 0044 0532 0130 00"],
        None,
    );
    gesehen.extend(stdout(&out).lines().map(str::to_string));

    let ausgabe = gesehen.join("\n");
    for wortlaut in [
        "nicht entpackt — ",
        "nicht durchsucht — Verschachtelungstiefe ",
    ] {
        assert!(
            ausgabe.contains(wortlaut),
            "die Ausgabe kennt „{wortlaut}“ nicht mehr:\n{ausgabe}"
        );
        assert!(
            security.contains(wortlaut.trim_end()),
            "SECURITY.md nennt den Grund „{wortlaut}“ nicht, den der Lauf schreibt"
        );
    }
    // Die Gründe, die nur ohne den Ladeschritt erreichbar sind, stehen
    // weiter in der Tabelle — das Orakel schreibt sie unverändert.
    for wortlaut in [
        "Sicht 7 (Schriftdekoder) nicht gelaufen:",
        "die Vorprüfung des Laders lehnt die Datei ab:",
    ] {
        assert!(
            security.contains(wortlaut),
            "SECURITY.md nennt den Grund „{wortlaut}“ nicht mehr"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// E5 der Fix-Runde 7 — die Zahl im Ergebnissatz zählt Stellen, nicht Zeilen
// ---------------------------------------------------------------------------

/// Eine Datei mit `n` getarnten zlib-Strömen, jeder so groß, dass ihn ein
/// kleines Budget nicht entpackt.
///
/// Ohne `/Filter`: die Vorprüfung des Laders zählt die **Rohbytes** (wenige
/// Kilobyte je Strom) und lässt die Datei durch; die Rohsicht der Nachprüfung
/// bläst jeden auf und lehnt ihn am Budget ab. Damit stehen `n` Stellen in
/// `unchecked` — mehr, als die gedeckelte Liste einzeln nennen kann.
fn pdf_mit_vielen_zu_grossen_stroemen(n: u32, megabytes: usize) -> Vec<u8> {
    use lopdf::{dictionary, Stream};

    let mut objekte: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R >>".to_vec(),
        ),
        (4, b"<< /Length 5 >>\nstream\nBT ET\nendstream".to_vec()),
    ];
    for i in 0..n {
        let mut stream = Stream::new(dictionary! {}, vec![b'A'; megabytes * 1024 * 1024]);
        stream.compress().expect("komprimierbar");
        let mut blob = format!("<< /Length {} >>\nstream\n", stream.content.len()).into_bytes();
        blob.extend_from_slice(&stream.content);
        blob.extend_from_slice(b"\nendstream");
        objekte.push((5 + i, blob));
    }
    assemble(&objekte)
}

/// **Befund R2-B der Gegenprüfung 6.** Der Ergebnissatz nannte die Zahl der
/// **Zeilen** (`LeakCheck::unchecked.len()`) und stand damit unter einer
/// Liste, die von mehr Stellen sprach als er: über der Decke
/// `MAX_UNCHECKED = 50` fasst `redact-pdf` den Rest in eine Summenzeile
/// („… und N weitere“), und die Zeilenzahl ist dann **kleiner** als die Zahl
/// der Stellen.
///
/// Gemessen wird der Widerspruch selbst, nicht eine feste Zahl: die Zahl im
/// Satz muss mindestens so groß sein wie die Zahl der genannten Zeilen **plus**
/// der Rest, den die Summenzeile zählt.
///
/// Mutation (nachgewiesen): in `check::report` `check.unchecked_places` zurück
/// auf `check.unchecked.len()` → dieser Test ist rot.
#[test]
fn die_zahl_im_ergebnissatz_zaehlt_stellen_nicht_zeilen() {
    let dir = workdir("stellen");
    let stroeme = 57u32;
    std::fs::write(
        dir.join("viele.pdf"),
        pdf_mit_vielen_zu_grossen_stroemen(stroeme, 2),
    )
    .unwrap();

    let out = run_in(
        &dir,
        &[
            "viele.pdf",
            "--check-leaks",
            "GEHEIM",
            "--max-decompressed-mb",
            "1",
        ],
        None,
    );
    let text = stdout(&out);
    assert_eq!(
        out.status.code(),
        Some(3),
        "ungeprüfte Stellen müssen 3 ergeben:\n{text}"
    );

    let zeilen: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|z| z.starts_with("NICHT GEPRÜFT:"))
        .collect();
    assert!(
        zeilen.len() > 10,
        "nur {} NICHT-GEPRÜFT-Zeile(n) — die Datei löst die Decke nicht aus:\n{text}",
        zeilen.len()
    );
    // Die Summenzeile: „… und N weitere Ströme nicht entpackt“.
    let weitere: usize = zeilen
        .iter()
        .find_map(|z| {
            let ab = z.find("… und ")? + "… und ".len();
            z[ab..].split_whitespace().next()?.parse::<usize>().ok()
        })
        .unwrap_or_else(|| panic!("keine Summenzeile in der Liste:\n{text}"));
    assert!(weitere > 0, "die Summenzeile zählt nichts:\n{text}");

    println!(
        "{} NICHT-GEPRÜFT-Zeile(n), Summenzeile zählt {weitere} weitere",
        zeilen.len()
    );
    let gemeldet: usize = text
        .lines()
        .find_map(|z| {
            let rest = z.trim().strip_prefix("Ergebnis: ")?;
            let zahl = rest.strip_suffix(rest.split_once(" Stelle(n) nicht geprüft")?.1)?;
            zahl.trim_end_matches(" Stelle(n) nicht geprüft")
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or_else(|| panic!("kein Ergebnissatz mit Zahl:\n{text}"));

    // Die Zeilen, die eine einzelne Stelle nennen, sind alle außer der
    // Summenzeile; dazu kommen die `weitere`, die sie zusammenfasst.
    let mindestens = zeilen.len() - 1 + weitere;
    assert!(
        gemeldet >= mindestens,
        "der Satz sagt „{gemeldet} Stelle(n) nicht geprüft“, die Liste darüber nennt \
         {} Zeile(n) und zählt {weitere} weitere — die Zahl ist kleiner als das, was \
         über ihr steht:\n{text}",
        zeilen.len()
    );
    assert!(
        gemeldet > zeilen.len(),
        "der Satz zählt weiter Zeilen ({gemeldet}) statt Stellen:\n{text}"
    );
    println!("Ergebnissatz nennt {gemeldet} Stelle(n)");

    std::fs::remove_dir_all(&dir).ok();
}
