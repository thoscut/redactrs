//! `--check-leaks` am gebauten Binary.
//!
//! ## Warum diese Tests das Binary starten und nicht die Funktion aufrufen
//!
//! Der Befund, der zu diesem Schalter geführt hat, lautete: die Prüfung war
//! **im Quelltext** vorhanden und **im Release** nicht erreichbar. Ein Test,
//! der `redact_pdf::leaks` aufruft, hätte diesen Befund nicht bemerkt — er
//! wäre grün geblieben, während der Downloader vor einer Anleitung stand, die
//! eine Rust-Toolchain und einen Klon eines privaten Repositories verlangte.
//! Geprüft wird deshalb genau das, was ausgeliefert wird: der Prozess, seine
//! Ausgabe und sein Rückgabewert.
//!
//! ## Die Auflage
//!
//! Vier Zusagen hängen an diesem Schalter, und jede hat hier ihren Test:
//!
//! 1. **Er findet, was `pdftotext … | grep …` übersieht.** Nachgestellt an der
//!    eigenen Demo (`the_four_secrets_of_the_demo_are_found`) und an einem
//!    Flate-komprimierten Objektstrom, in den weder `pdftotext` noch `strings`
//!    hineinsehen (`a_secret_in_a_compressed_object_stream_is_found`).
//! 2. **„Nichts gefunden“ wird nicht zur Entwarnung.** Der Satz dazu steht in
//!    der Ausgabe, und zwar auf stdout (`a_clean_run_says_what_it_does_not_prove`).
//! 3. **Ein Fund ist Rückgabewert 3**, kein Fehler und kein Erfolg.
//! 4. **Die Eingabedatei geht durch dieselbe Tür wie jede andere**: Typ- und
//!    Größenprüfung vor dem ersten gelesenen Byte, also auch keine benannte
//!    Pipe, die den Lauf anhält.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "redact-check-leaks-{}-{name}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Ein Lauf ohne Einstellungsdatei und ohne Passwort aus der Umgebung — die
/// Umgebung des Testläufers darf das Ergebnis nicht verschieben.
fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary startbar")
}

/// Wie [`run`], aber mit einer Begriffsliste auf der Standardeingabe.
fn run_with_stdin(args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Binary startbar");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("schreibbar");
    child.wait_with_output().expect("Lauf endet")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("Prozess endet mit Rückgabewert")
}

/// Steht `needle` in einer Kopfzeile „GEFUNDEN (…): <needle>“?
///
/// Bewusst zeilenweise und am Zeilenende festgemacht: das Geheimnis kommt
/// auch in der *Umgebung* jeder Fundstelle vor, ein `text.contains(needle)`
/// wäre also schon dann wahr, wenn ein ganz anderer Begriff gefunden wurde.
fn reported_as_found(report: &str, needle: &str) -> bool {
    report
        .lines()
        .any(|line| line.trim_start().starts_with("GEFUNDEN (") && line.ends_with(needle))
}

/// Die Demo-Ausgabe aus der README: geschwärzt mit `--patterns
/// iban_de,bic,email` — IBAN und BIC sind weg, Name, Kontonummer,
/// Telefonnummer und Steuer-ID stehen noch darin. Genau die Datei, an der
/// `pdftotext … | grep DE89` „sauber“ meldet.
fn demo_redacted_with_three_patterns(dir: &Path) -> PathBuf {
    let input = dir.join("kontoauszug.pdf");
    std::fs::write(&input, redact_pdf::testing::demo_statement()).unwrap();
    let out = dir.join("teilweise.pdf");
    let run = run(&[
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--patterns",
        "iban_de,bic,email",
    ]);
    assert_eq!(code(&run), 0, "Vorlauf: {}", stderr(&run));
    out
}

/// Dieselbe Demo mit den Vorgabemustern — die Gegenprobe.
fn demo_redacted_with_defaults(dir: &Path) -> PathBuf {
    let input = dir.join("kontoauszug2.pdf");
    std::fs::write(&input, redact_pdf::testing::demo_statement()).unwrap();
    let out = dir.join("vorgabe.pdf");
    let run = run(&[input.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code(&run), 0, "Vorlauf: {}", stderr(&run));
    out
}

// ---------------------------------------------------------------------------
// Die Kernzusage: es findet, was der naive Weg übersieht
// ---------------------------------------------------------------------------

/// **Die Auflage.** An der eigenen Demo meldet `pdftotext … | grep DE89`
/// „kein Treffer“ — und in derselben Datei stehen vier Geheimnisse.
/// `--check-leaks` findet alle vier und endet mit 3.
///
/// Der Vergleich mit `pdftotext` selbst steht nicht hier: das Werkzeug gehört
/// nicht zum Baum und wäre auf `windows-2025` nicht vorhanden. Nachgemessen
/// ist er von Hand (poppler 24.02.0) und in der README festgehalten. Was
/// dieser Test sichert, ist die Hälfte, die im Baum liegt: dass die vier
/// **gefunden** werden.
#[test]
fn the_four_secrets_of_the_demo_are_found() {
    let dir = workdir("vier");
    let pdf = demo_redacted_with_three_patterns(&dir);

    let out = run(&[
        pdf.to_str().unwrap(),
        "--check-leaks",
        "Max Mustermann",
        "--check-leaks",
        "532013000",
        "--check-leaks",
        "+49 30 123456789",
        "--check-leaks",
        "12345678901",
    ]);
    let text = stdout(&out);
    assert_eq!(code(&out), 3, "ein Fund ist Rückgabewert 3:\n{text}");
    for secret in [
        "Max Mustermann",
        "532013000",
        "+49 30 123456789",
        "12345678901",
    ] {
        assert!(
            reported_as_found(&text, secret),
            "{secret} ist nicht als Fund gemeldet:\n{text}"
        );
    }
    assert!(
        text.contains("4 von 4"),
        "die Zusammenfassung zählt falsch:\n{text}"
    );
    // Und die Fundstelle sagt, *wo* — sonst wäre der Bericht nicht prüfbar.
    assert!(text.contains("Objekt"), "keine Fundorte genannt:\n{text}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Gegenprobe, die dem Ergebnis erst seinen Wert gibt: dieselbe Demo mit
/// den Vorgabemustern. Vier der fünf Werte sind restlos weg, der Name steht
/// noch da — für Namen gibt es kein Muster.
///
/// Ohne diesen Test wäre der vorige nur der Nachweis, dass die Prüfung
/// überhaupt etwas meldet. Erst zusammen zeigen sie, dass sie **unterscheidet**.
#[test]
fn with_the_default_patterns_only_the_name_is_left() {
    let dir = workdir("gegenprobe");
    let pdf = demo_redacted_with_defaults(&dir);

    for clean in [
        "DE89 3704 0044 0532 0130 00",
        "532013000",
        "+49 30 123456789",
        "12345678901",
    ] {
        let out = run(&[pdf.to_str().unwrap(), "--check-leaks", clean]);
        assert_eq!(
            code(&out),
            0,
            "{clean} steht noch in der Datei:\n{}",
            stdout(&out)
        );
    }
    let out = run(&[pdf.to_str().unwrap(), "--check-leaks", "Max Mustermann"]);
    assert_eq!(
        code(&out),
        3,
        "der Name müsste stehen bleiben:\n{}",
        stdout(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die zweite Kernzusage:** ein Flate-komprimierter Objektstrom ist für eine
/// Rohbyte-Suche unsichtbar. `strings … | grep` und `grep` auf der Datei
/// selbst finden hier nichts — der Test stellt beide nach und verlangt vom
/// Werkzeug das Gegenteil.
#[test]
fn a_secret_in_a_compressed_object_stream_is_found() {
    let dir = workdir("objstm");
    let secret = "DE89 3704 0044 0532 0130 00";
    let pdf = dir.join("objstm.pdf");
    std::fs::write(&pdf, object_stream_pdf(secret)).unwrap();

    // Erst die Gegenprobe: in den Rohbytes steht das Geheimnis nicht.
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(
        !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
        "die Datei taugt nicht als Prüfstein — das Geheimnis steht im Klartext darin"
    );

    let out = run(&[pdf.to_str().unwrap(), "--check-leaks", secret]);
    let text = stdout(&out);
    assert_eq!(code(&out), 3, "im Objektstrom übersehen:\n{text}");
    assert!(text.contains("ObjStm"), "der Fundort fehlt:\n{text}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein winziges PDF, dessen einziges Geheimnis in einem Flate-komprimierten
/// `/ObjStm` steckt.
///
/// **Von Hand zusammengesetzt, nicht über `Document::save_to`.** Ein
/// Schreiber darf einen Objektstrom auflösen und seine Objekte einzeln
/// ablegen — dann stünde das Geheimnis im Klartext in der Datei, und der Test
/// prüfte etwas anderes, als er behauptet. Hier steht Byte für Byte, was
/// geprüft wird. Komprimiert wird über `lopdf::Stream::compress`, damit dafür
/// keine weitere Abhängigkeit nötig ist.
fn object_stream_pdf(secret: &str) -> Vec<u8> {
    use lopdf::{dictionary, Stream};

    // Der Kopf eines /ObjStm ist „<Objekt-Id> <Versatz>“ je Objekt, danach
    // folgen die Objekte selbst.
    //
    // Das zweite Objekt ist Füllung, und zwar aus einem Grund: `deflate`
    // speichert eine Handvoll Bytes **unkomprimiert** (stored block). Ein
    // /ObjStm mit nur der kurzen Zeichenkette darin trüge das Geheimnis
    // deshalb im Klartext, und der Test prüfte das Gegenteil dessen, was er
    // behauptet. Mit einer gut komprimierbaren Füllung dahinter wählt der
    // Kodierer einen Huffman-Block, und kein Byte des Geheimnisses steht mehr
    // so in der Datei. Die Zusicherung darüber steht als Gegenprobe im Test.
    let hidden = format!("({secret})");
    let filler = format!("({})", "A".repeat(4096));
    let head = format!("9 0 10 {} ", hidden.len() + 1);
    let payload = format!("{head}{hidden} {filler}");
    let mut stream = Stream::new(dictionary! {}, payload.into_bytes());
    stream.compress().expect("komprimierbar");
    let compressed = stream.content;

    let objects: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
        ),
        (4, stream_object(b"BT ET")),
        (8, {
            let mut dict = format!(
                "<< /Type /ObjStm /N 2 /First {} /Filter /FlateDecode /Length {} >>\nstream\n",
                head.len(),
                compressed.len()
            )
            .into_bytes();
            dict.extend_from_slice(&compressed);
            dict.extend_from_slice(b"\nendstream");
            dict
        }),
    ];

    let mut out = b"%PDF-1.5\n".to_vec();
    let mut offsets = std::collections::BTreeMap::new();
    for (id, body) in &objects {
        offsets.insert(*id, out.len());
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.iter().map(|(id, _)| *id).max().unwrap() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..size {
        match offsets.get(&id) {
            Some(at) => out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn stream_object(content: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    out.extend_from_slice(content);
    out.extend_from_slice(b"\nendstream");
    out
}

// ---------------------------------------------------------------------------
// „Nichts gefunden“ ist kein Freibrief
// ---------------------------------------------------------------------------

/// **Die Auflage:** ein sauberer Lauf sagt dazu, was er *nicht* bewiesen hat.
///
/// Und er sagt es auf **stdout**: `redact-rs … --check-leaks … > bericht.txt`
/// behielte sonst genau die Hälfte, die wie eine Freigabe aussieht.
#[test]
fn a_clean_run_says_what_it_does_not_prove() {
    let dir = workdir("freibrief");
    let pdf = demo_redacted_with_defaults(&dir);

    let out = run(&[
        pdf.to_str().unwrap(),
        "--check-leaks",
        "DE89 3704 0044 0532 0130 00",
    ]);
    let text = stdout(&out);
    assert_eq!(code(&out), 0, "{text}");
    assert!(
        text.contains("NICHT"),
        "der Vorbehalt fehlt in der Ausgabe:\n{text}"
    );
    assert!(
        text.contains("genau diese"),
        "der Vorbehalt nennt den Grund nicht:\n{text}"
    );
    assert!(
        !stderr(&out).contains("NICHT"),
        "der Vorbehalt gehört auf stdout, nicht auf stderr"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// `--quiet` schweigt über einen sauberen Lauf, **nicht** über einen Fund.
/// Wer die Ausgabe abschaltet, verzichtet auf die Zusammenfassung — nicht auf
/// die Nachricht, wegen der er geprüft hat.
#[test]
fn quiet_keeps_the_finding() {
    let dir = workdir("leise");
    let pdf = demo_redacted_with_three_patterns(&dir);

    let clean = run(&[
        pdf.to_str().unwrap(),
        "--quiet",
        "--check-leaks",
        "DE89 3704 0044 0532 0130 00",
    ]);
    assert_eq!(code(&clean), 0);
    assert!(
        stdout(&clean).is_empty(),
        "still heißt still:\n{}",
        stdout(&clean)
    );

    let leak = run(&[
        pdf.to_str().unwrap(),
        "--quiet",
        "--check-leaks",
        "Max Mustermann",
    ]);
    assert_eq!(code(&leak), 3);
    assert!(
        stdout(&leak).contains("Max Mustermann"),
        "ein Fund darf nicht verschwiegen werden:\n{}",
        stdout(&leak)
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Vertraulichkeit: die Begriffe von der Standardeingabe
// ---------------------------------------------------------------------------

/// **Die Auflage:** die Suchbegriffe müssen an der Prozessliste und der
/// Shell-Historie vorbeikommen. `-` liest sie zeilenweise von stdin.
///
/// Nachgewiesen wird hier, dass der Weg dieselbe Antwort liefert wie die
/// Kommandozeile — dass er die Begriffe aus `argv` heraushält, sagt schon die
/// Form des Aufrufs: in ihm steht nur `-`.
#[test]
fn the_needles_may_come_from_standard_input() {
    let dir = workdir("stdin");
    let pdf = demo_redacted_with_three_patterns(&dir);

    let out = run_with_stdin(
        &[pdf.to_str().unwrap(), "--check-leaks", "-"],
        // Leerzeile und Windows-Zeilenende sind Alltag in einer von Hand
        // geschriebenen Liste und dürfen die Prüfung nicht verschieben.
        "DE89 3704 0044 0532 0130 00\r\n\nMax Mustermann\n",
    );
    let text = stdout(&out);
    assert_eq!(code(&out), 3, "{text}");
    assert!(text.contains("nicht gefunden: DE89"), "{text}");
    assert!(text.contains("GEFUNDEN"), "{text}");
    assert!(
        text.contains("1 von 2"),
        "die leere Zeile wurde als Begriff gezählt:\n{text}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Eine leere Standardeingabe ist keine Prüfung, sondern ein Bedienfehler —
/// „0 Begriffe geprüft, nichts gefunden“ wäre die gefährlichste aller
/// Entwarnungen.
#[test]
fn an_empty_needle_list_is_a_usage_error() {
    let dir = workdir("leer");
    let pdf = demo_redacted_with_defaults(&dir);

    let out = run_with_stdin(&[pdf.to_str().unwrap(), "--check-leaks", "-"], "\n\n  \n");
    assert_eq!(code(&out), 2, "{}", stdout(&out));
    assert!(
        stderr(&out).contains("ohne Suchbegriff"),
        "{}",
        stderr(&out)
    );

    let out = run(&[pdf.to_str().unwrap(), "--check-leaks", "   "]);
    assert_eq!(code(&out), 2, "{}", stdout(&out));
    assert!(stderr(&out).contains("leerem Text"), "{}", stderr(&out));

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Die Eingabedatei geht durch dieselbe Tür wie jede andere
// ---------------------------------------------------------------------------

/// Keine PDF-Datei: derselbe Satz wie beim Schwärzen, Rückgabewert 1.
///
/// Wichtig ist die *Richtung* des Fehlers: eine beliebige Datei einfach zu
/// durchsuchen wäre bequem und gefährlich — wer versehentlich die falsche
/// Datei angibt, bekäme „nicht gefunden“ und hielte das für eine Auskunft
/// über sein PDF.
#[test]
fn a_file_that_is_not_a_pdf_is_refused() {
    let dir = workdir("kein-pdf");
    let path = dir.join("notiz.txt");
    std::fs::write(&path, b"Max Mustermann steht hier im Klartext\n").unwrap();

    let out = run(&[path.to_str().unwrap(), "--check-leaks", "Max Mustermann"]);
    assert_eq!(code(&out), 1, "{}", stdout(&out));
    let err = stderr(&out);
    assert!(err.contains("keine PDF-Datei"), "{err}");
    assert!(err.contains("notiz.txt"), "die Datei wird benannt: {err}");

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** eine benannte Pipe hält den Lauf nicht an.
///
/// Der Test kommt ohne Schreiber am anderen Ende aus, und das ist der Punkt:
/// schon das Öffnen einer Pipe ohne Schreiber blockiert endlos. Kehrt dieser
/// Test zurück, ist vor dem Öffnen entschieden worden.
///
/// `cfg(unix)` sagt, dass es benannte Pipes gibt — nicht, dass `mkfifo` im
/// `PATH` steht. Auf einem schlanken Unix-Bild ohne die Werkzeuge (BusyBox
/// ohne `mkfifo`, ein Container mit `scratch`-Basis) ließ
/// `expect("mkfifo startbar")` den Testlauf platzen, obwohl am Programm nichts
/// falsch war — derselbe Fehler, den `belege.rs` bei `python3` schon
/// vermeidet: fehlt das Werkzeug, ist hier nichts zu prüfen, und der Test
/// **sagt das** und endet grün. Ein **vorhandenes** `mkfifo`, das scheitert,
/// bleibt dagegen ein Fehler: dann gibt es die Pipe, und die Prüfung wäre
/// klammheimlich ausgefallen.
#[cfg(unix)]
#[test]
fn a_named_pipe_is_refused_without_opening_it() {
    let dir = workdir("pipe");
    let path = dir.join("pipe.pdf");
    let ok = match Command::new("mkfifo").arg(&path).status() {
        Ok(status) => status,
        Err(e) => {
            eprintln!(
                "kein mkfifo im Pfad ({e}) — dass `--check-leaks` eine benannte \
                 Pipe ablehnt, bleibt hier ungeprüft"
            );
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
    };
    assert!(ok.success(), "mkfifo ist fehlgeschlagen");

    let out = run(&[path.to_str().unwrap(), "--check-leaks", "DE89"]);
    assert_eq!(code(&out), 1, "{}", stdout(&out));
    assert!(
        stderr(&out).contains("keine gewöhnliche Datei"),
        "{}",
        stderr(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein verschlüsseltes Dokument wird abgelehnt, statt „nichts gefunden“ zu
/// melden. In einer verschlüsselten Datei stehen die Zeichenketten
/// verschlüsselt; eine Bytesuche fände dort auch dann nichts, wenn das
/// Geheimnis noch darin steht — das wäre die falscheste aller Antworten.
#[test]
fn an_encrypted_document_is_refused_instead_of_reported_clean() {
    let dir = workdir("verschluesselt");
    let path = dir.join("verschluesselt.pdf");
    std::fs::write(&path, redact_pipeline::testing::ENCRYPTED_PDF).unwrap();

    let out = run(&[
        path.to_str().unwrap(),
        "--check-leaks",
        redact_pipeline::testing::ENCRYPTED_PDF_IBAN,
    ]);
    assert_eq!(code(&out), 1, "{}", stdout(&out));
    let err = stderr(&out);
    assert!(err.contains("verschlüsselt"), "{err}");
    assert!(
        err.contains("fände auch dann nichts"),
        "der Grund fehlt: {err}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Steuerzeichen — im Suchbegriff **und** in der Datei — erreichen das
/// Terminal nicht. Ein Fundort aus einer fremden Datei darf die Ausgabe nicht
/// fälschen können; ausgerechnet hier liest jemand ab, ob eine Datei sauber
/// ist.
#[test]
fn control_characters_never_reach_the_terminal() {
    let dir = workdir("steuerzeichen");
    let pdf = demo_redacted_with_defaults(&dir);

    let needle = "Max\u{1b}[2K Mustermann";
    let out = run(&[pdf.to_str().unwrap(), "--check-leaks", needle]);
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(
        !stdout(&out).contains('\u{1b}'),
        "ein ESC ist durchgekommen:\n{:?}",
        stdout(&out)
    );

    // Und derselbe Weg über einen Fund: hier steht das Steuerzeichen in der
    // Datei und kommt zusätzlich in der Umgebung der Fundstelle vor.
    let hidden = dir.join("steuer.pdf");
    std::fs::write(&hidden, object_stream_pdf("GEHEIM\u{1b}[2Kweg")).unwrap();
    let out = run(&[
        hidden.to_str().unwrap(),
        "--check-leaks",
        "GEHEIM\u{1b}[2Kweg",
    ]);
    assert_eq!(code(&out), 3, "{}", stdout(&out));
    assert!(
        !stdout(&out).contains('\u{1b}'),
        "ein ESC aus der Datei ist durchgekommen:\n{:?}",
        stdout(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Bedienung
// ---------------------------------------------------------------------------

/// Genau eine Datei, und sie muss dastehen.
#[test]
fn exactly_one_file_is_checked() {
    let dir = workdir("eine-datei");
    let pdf = demo_redacted_with_defaults(&dir);

    let none = run(&["--check-leaks", "DE89"]);
    assert_eq!(code(&none), 2, "{}", stdout(&none));
    assert!(
        stderr(&none).contains("braucht die zu prüfende"),
        "{}",
        stderr(&none)
    );

    let two = run(&[
        pdf.to_str().unwrap(),
        pdf.to_str().unwrap(),
        "--check-leaks",
        "DE89",
    ]);
    assert_eq!(code(&two), 2, "{}", stdout(&two));
    assert!(
        stderr(&two).contains("genau eine Datei"),
        "{}",
        stderr(&two)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** `--check-leaks` mit einem Schalter des Schwärzens ist ein
/// Bedienfehler (2), kein stillschweigend übergangener Wunsch.
///
/// `-o` ist der wichtigste Fall: die Erwartung wäre „schwärzen **und**
/// nachprüfen“, das Ergebnis eine Datei, die nie geschrieben wird.
#[test]
fn switches_of_the_redaction_are_refused() {
    let dir = workdir("konflikte");
    let pdf = demo_redacted_with_defaults(&dir);
    let path = pdf.to_str().unwrap();

    for extra in [
        vec!["-o", "egal.pdf"],
        vec!["--review"],
        vec!["--audit-log", "egal.json"],
        vec!["--patterns", "iban_de"],
        vec!["--no-patterns"],
        vec!["--action", "replace"],
        vec!["--padding", "3"],
        vec!["--json"],
        vec!["--password", "geheim"],
        vec!["--write-demo", "egal.pdf"],
        vec!["--list-patterns"],
        // Begrenzt die dekodierten Bildbytes — hier wird kein Bild dekodiert.
        vec!["--max-image-mb", "8"],
    ] {
        let mut args = vec![path, "--check-leaks", "DE89"];
        args.extend(extra.iter().copied());
        let out = run(&args);
        assert_eq!(
            code(&out),
            2,
            "{extra:?} müsste abgelehnt werden:\n{}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("cannot be used with"),
            "{extra:?}: {}",
            stderr(&out)
        );
    }

    // Die Gegenprobe: die Grenzen und `--quiet` bleiben erlaubt — sie gelten
    // beim Lesen jeder fremden Datei.
    for extra in [
        vec!["--quiet"],
        vec!["--max-input-mb", "8"],
        vec!["--max-decompressed-mb", "64"],
    ] {
        let mut args = vec![path, "--check-leaks", "DE89"];
        args.extend(extra.iter().copied());
        let out = run(&args);
        assert_eq!(
            code(&out),
            0,
            "{extra:?} wurde abgelehnt:\n{}",
            stderr(&out)
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein Komma trennt **nicht**: „Mustermann, Max“ ist ein Suchbegriff.
///
/// `--patterns` benutzt `value_delimiter = ','`; würde dieser Schalter es
/// nachmachen, zerfiele jeder Name mit Komma in zwei Bruchstücke, und beide
/// fänden nichts — die Prüfung meldete „sauber“ für einen Namen, der dasteht.
#[test]
fn a_comma_belongs_to_the_needle() {
    let dir = workdir("komma");
    let input = dir.join("komma.pdf");
    std::fs::write(&input, object_stream_pdf("Mustermann, Max")).unwrap();

    let out = run(&[input.to_str().unwrap(), "--check-leaks", "Mustermann, Max"]);
    assert_eq!(code(&out), 3, "{}", stdout(&out));
    // Ein einzelner Begriff bekommt einen eigenen Satz — „1 von 1
    // Suchbegriffen stehen“ wäre weder Zahl noch Deutsch.
    assert!(
        stdout(&out).contains("der Suchbegriff steht noch in der Datei"),
        "{}",
        stdout(&out)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Der Hilfetext nennt den Schalter und sagt, was der Rückgabewert bedeutet —
/// das ist die einzige Anleitung, die dem Downloader ohne Quelltext bleibt.
#[test]
fn the_help_explains_the_switch_and_its_exit_code() {
    let out = run(&["--help"]);
    let help = stdout(&out);
    assert!(help.contains("--check-leaks"), "{help}");
    // Der Beispielteil zeigt beide Wege — auch den an der Prozessliste vorbei.
    assert!(help.contains("--check-leaks - <"), "{help}");
    assert!(
        help.contains("--check-leaks hat mindestens einen Begriff"),
        "die Erklärung des Rückgabewerts fehlt:\n{help}"
    );
}
