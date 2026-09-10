//! Gegenprüfung Q5 der Fix-Runde 5: **die Regel, die das lokale Tor nicht
//! fahren kann.**
//!
//! Zweimal war der Windows-Job der CI rot, und beide Male durch einen *Test*,
//! nicht durch das Programm: die Speichermessung der Bombentests las auf jedem
//! Ziel `/proc/self/status`. Das lokale Tor fährt für Windows nur
//! `cargo clippy --target x86_64-pc-windows-gnu`; Clippy prüft (mit
//! `--all-targets`) auch den Testcode, aber es **führt** ihn nicht aus. Der
//! Fehler war kein Typfehler, sondern ein Laufzeitfehler — Clippy konnte ihn
//! nicht sehen.
//!
//! Nachgemessen, ob sich die Lücke schließen lässt, indem das Tor die Tests
//! für Windows wenigstens **baut**:
//!
//! ```text
//! $ cargo test --no-run --target x86_64-pc-windows-gnu -p redact-cli
//! error: error calling dlltool 'x86_64-w64-mingw32-dlltool':
//!        No such file or directory (os error 2)
//! ```
//!
//! Mit `llvm-dlltool` (liegt in `/usr/bin`) als `x86_64-w64-mingw32-dlltool`
//! im Pfad kommt der Bau weiter und bleibt am nächsten Werkzeug stehen:
//!
//! ```text
//! error: linker `x86_64-w64-mingw32-gcc` not found
//! ```
//!
//! Der `windows-gnu`-Zweig braucht die mingw-Laufzeit (`libmingwex.a` und
//! Genossen); die Rust-Toolchain bringt nur `crt2.o` und `dllcrt2.o` mit
//! (`lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained/`), `apt list
//! --installed | grep mingw` findet nichts, und `/var/cache/apt/archives`
//! ist leer — ohne Netz ist hier nichts nachzuinstallieren. Ein
//! `cargo test --no-run` für Windows ist im lokalen Tor also **nicht** zu
//! haben.
//!
//! Was zu haben ist, ist eine Regel **im Baum**, die genau die Fehlerklasse
//! ausschließt, die zweimal durchkam: eine Stelle, die eine Einrichtung des
//! ausführenden Systems beim Namen nennt (`/proc/…`, `/sys/…`, `/dev/…`,
//! `ulimit`), muss entweder
//!
//! * hinter einem Plattform-`cfg` stehen (`#![cfg(target_os = "linux")]`,
//!   `#[cfg(unix)]`, `cfg!(target_os = "linux")`) — dann entsteht sie auf
//!   anderen Zielen gar nicht —, **oder**
//! * ihr Ergebnis als *optional* behandeln (`.ok()`, `unwrap_or…`,
//!   `is_err()`, `map_or`, `if let Ok(…)`) — dann fehlt auf anderen Zielen
//!   die Zahl und nicht der Test.
//!
//! Beides ist erlaubt, weil beides trägt. Verboten ist die dritte Fassung,
//! die zweimal rot war: die Einrichtung ungeschützt nennen und ihr Ergebnis
//! als gegeben nehmen (`.expect(…)`, `.unwrap()`).
//!
//! Der Test liest den Quelltext, nicht das Programm — deshalb steht er hier
//! und braucht kein Windows. Nachweis, dass er beißt (Mutation, in der
//! Gegenprüfung gefahren): in `crates/redact-pdf/tests/rev4_bench.rs` das
//! `.ok()` hinter `read_to_string("/proc/self/status")` gestrichen und durch
//! `.expect("…")` ersetzt → dieser Test ist rot und nennt Datei und Zeile.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Einrichtungen des ausführenden Systems, die es nur unter Unix (meist nur
/// unter Linux) gibt — als **Zeichenkettenliteral** gesucht, damit ein Satz
/// im Fließtext eines Kommentars nicht mitzählt.
const HEIKEL: &[&str] = &["\"/proc/", "\"/sys/", "\"/dev/", "\"ulimit\""];

/// Ein Plattform-`cfg` in der Nähe: dann entsteht die Stelle auf anderen
/// Zielen nicht.
const CFG: &[&str] = &[
    "#![cfg(target_os",
    "#![cfg(unix",
    "#![cfg(target_family",
    "#[cfg(target_os",
    "#[cfg(unix",
    "#[cfg(target_family",
    "cfg!(target_os",
    "cfg!(unix",
    "cfg!(target_family",
];

/// Das Ergebnis wird als *optional* behandelt: fehlt die Einrichtung, fehlt
/// die Zahl und nicht der Test.
const OPTIONAL: &[&str] = &[
    ".ok()",
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
    ".is_ok()",
    ".is_err()",
    "map_or",
    "if let Ok(",
    "= .. else",
    "let Ok(",
];

/// Wie viele Zeilen um die Stelle herum als „in der Nähe“ gelten. Groß genug
/// für eine `#[cfg(…)]`-Zeile über einer Funktion samt Doc-Kommentar wäre zu
/// groß — deshalb wird das Attribut zusätzlich über die *umgebende Funktion*
/// gesucht (siehe [`funktionskopf`]).
const NAHE: usize = 8;

/// Alle `*.rs` unter `crates/`.
fn rust_dateien(wurzel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(eintraege) = std::fs::read_dir(wurzel) else {
        return;
    };
    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();
        if pfad.is_dir() {
            if pfad.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_dateien(&pfad, out);
        } else if pfad.extension().is_some_and(|e| e == "rs") {
            out.push(pfad);
        }
    }
}

/// Die Zeile des Kopfes der Funktion, in der `zeile` steht (0-basiert), oder
/// `None`, wenn keine gefunden wird. Gesucht wird aufwärts nach der ersten
/// Zeile, die mit `fn `, `pub fn ` oder `    fn ` beginnt.
fn funktionskopf(zeilen: &[&str], zeile: usize) -> Option<usize> {
    (0..=zeile).rev().find(|&i| {
        let s = zeilen[i].trim_start();
        s.starts_with("fn ") || s.starts_with("pub fn ") || s.starts_with("pub(crate) fn ")
    })
}

/// Trägt die Funktion, in der die Stelle steht, ein Plattform-`cfg` als
/// Attribut? Die Attribute stehen direkt über dem Kopf, unterbrochen nur von
/// Doc-Kommentaren.
fn kopf_hat_cfg(zeilen: &[&str], kopf: usize) -> bool {
    for i in (0..kopf).rev() {
        let s = zeilen[i].trim_start();
        if CFG.iter().any(|c| s.starts_with(c)) {
            return true;
        }
        if s.starts_with("///") || s.starts_with("//") || s.starts_with("#[") || s.is_empty() {
            continue;
        }
        return false;
    }
    false
}

/// **Die Regel.** Jede Stelle im Baum, die `/proc`, `/sys`, `/dev` oder
/// `ulimit` beim Namen nennt, steht unter einem Plattform-`cfg` **oder**
/// behandelt ihr Ergebnis als optional.
///
/// Grün auf dem Stand `2b92bee`: zehn Stellen, keine davon ungeschützt mit
/// festem Ergebnis.
#[test]
fn keine_systemeinrichtung_ohne_cfg_oder_ohne_ausweg() {
    let crates = repo_root().join("crates");
    let mut dateien = Vec::new();
    rust_dateien(&crates, &mut dateien);
    assert!(
        dateien.len() > 50,
        "der Baum wurde nicht gefunden — nur {} Dateien unter {}",
        dateien.len(),
        crates.display()
    );

    let mut geprueft = 0usize;
    let mut verstoesse: Vec<String> = Vec::new();

    for datei in &dateien {
        // Diese Datei selbst nicht: sie *nennt* die Namen, um nach ihnen zu
        // suchen. Sonst prüfte die Regel ihren eigenen Wortlaut.
        if datei
            .file_name()
            .is_some_and(|n| n == "zf_q5_plattformzusagen.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(datei)
            .unwrap_or_else(|e| panic!("{} lesbar: {e}", datei.display()))
            .replace("\r\n", "\n");
        let zeilen: Vec<&str> = text.lines().collect();
        let datei_gedeckt = zeilen.iter().any(|z| {
            let s = z.trim_start();
            s.starts_with("#![cfg(target_os")
                || s.starts_with("#![cfg(unix")
                || s.starts_with("#![cfg(target_family")
        });

        for (i, zeile) in zeilen.iter().enumerate() {
            let getrimmt = zeile.trim_start();
            // Kommentare zählen nicht: dort *steht* nichts, dort steht etwas
            // *über* etwas.
            if getrimmt.starts_with("//") {
                continue;
            }
            if !HEIKEL.iter().any(|t| zeile.contains(t)) {
                continue;
            }
            geprueft += 1;
            if datei_gedeckt {
                continue;
            }
            let von = i.saturating_sub(NAHE);
            let bis = (i + NAHE + 1).min(zeilen.len());
            let davor = zeilen[von..=i].join("\n");
            let umfeld = zeilen[i..bis].join("\n");
            let unter_cfg = CFG.iter().any(|c| davor.contains(c))
                || funktionskopf(&zeilen, i).is_some_and(|k| kopf_hat_cfg(&zeilen, k));
            let hat_ausweg = OPTIONAL.iter().any(|o| umfeld.contains(o));
            if !unter_cfg && !hat_ausweg {
                verstoesse.push(format!(
                    "{}:{}  {}",
                    datei
                        .strip_prefix(repo_root())
                        .unwrap_or(datei)
                        .display()
                        .to_string()
                        .replace("../../", ""),
                    i + 1,
                    getrimmt
                ));
            }
        }
    }

    // Eine Regel, die nichts findet, prüft nichts. Der Baum nennt
    // `/proc/self/status` an mehreren Stellen — bleibt der Zähler klein, ist
    // die Suche kaputt und nicht der Baum sauber.
    assert!(
        geprueft >= 8,
        "nur {geprueft} Stelle(n) untersucht — die Suche greift nicht mehr"
    );
    assert!(
        verstoesse.is_empty(),
        "{} Stelle(n) nennen eine Einrichtung des ausführenden Systems, ohne \
         Plattform-cfg und ohne Ausweg für andere Ziele — genau daran war der \
         Windows-Job der CI zweimal rot:\n{}",
        verstoesse.len(),
        verstoesse.join("\n")
    );
}
