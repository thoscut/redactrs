//! Nach der Fix-Runde 9, Register #59: **das Gate passt wieder auf die
//! Platte — und die Stufe, die das bewirkt, ist gebunden.**
//!
//! Das Gate (`cargo test --workspace` und die vier Läufe daneben) passte nicht
//! mehr in die Plattenzuteilung des Prüfrechners: zweimal „No space left on
//! device“, einmal mitten in einen Schreibvorgang. Gemessen, wohin der Platz
//! ging — und es war kein Ausreißer, sondern jedes Testziel: das Binary
//! dieser Belegdatei (`zm_b_gleichzeitige_exporte`, egui gelinkt) war vorher
//! 240,7 MB groß, davon 219,4 MB in `.debug_*`-Sektionen — 91 %. Jedes der
//! Testbinaries trug seine eigene volle Kopie der Debug-Information aller
//! Abhängigkeiten, und der Workspace hatte nur `[profile.release]`.
//!
//! Der Knopf: `[profile.dev] debug = "line-tables-only"` in der
//! Workspace-`Cargo.toml`. `dev`, nicht `test`, weil `cargo test` die
//! Abhängigkeiten mit `dev` baut und `test` von `dev` erbt. Dasselbe Binary
//! danach: 78,0 MB, davon 58,3 MB `.debug_*` — die Zeilentabellen bleiben
//! (`.debug_line` 21,6 MB), Backtraces nennen weiter Datei:Zeile.
//!
//! **Die Stufe ändert keinen Maschinencode.** `debug` steuert nur die
//! DWARF-Ausgabe; `opt-level`, `debug-assertions` und `overflow-checks`
//! bleiben, und `.debug_*`-Sektionen sind nicht `SHF_ALLOC`, also nie im
//! Arbeitsspeicher. Keine gebundene Zeit- oder Speicher-Messzahl hängt daran
//! — die Gegenbehauptung im Commit `1282b16` war falsch und ist im CHANGELOG
//! zurückgenommen.
//!
//! Zwei Bindungen, an die Zeile und an die Wirkung:
//!
//! * [`die_stufe_steht_im_manifest`] liest die Workspace-`Cargo.toml` und
//!   verlangt die Zeile im `[profile.dev]`-Block. Mutation: Zeile entfernen →
//!   rot, ohne Neubau.
//! * [`das_eigene_binary_traegt_keine_volle_debug_info`] misst die Größe des
//!   Binaries, in dem es selbst läuft, gegen eine Decke von [`DECKE_MB`] MB.
//!   Gemessen an diesem Binary: 111 MB mit Zeilentabellen, 272 MB unter der
//!   Mutation (Knopf raus, Neubau; Skript `mutation_zn_a.sh`, Zurücknahme aus
//!   einer Sicherungskopie) — die Decke liegt dazwischen, mit Abstand nach
//!   beiden Seiten. Nur unter Linux: die
//!   Größe der ELF-Datei ist eine Einrichtung des Systems
//!   (`zf_q5_plattformzusagen`); unter MSVC liegt die Debug-Information
//!   ohnehin in der `.pdb` neben der `.exe`.
//!
//! **Ein Scheintest, den der erste Lauf entlarvt hat:** die erste Fassung
//! dieses Tests las nur `Cargo.toml` und ihre eigene Datei — und war 6 MB
//! groß. Ein Test, der nichts aus `redact_gui` ruft, linkt auch nichts davon;
//! der Linker wirft Unbenutztes weg, und die Decke hätte jede Mutation
//! überlebt. Deshalb nimmt [`egui_ist_gelinkt`] einen Funktionszeiger auf
//! [`redact_gui::run`], den Einstieg von eframe: ein Zeiger muss auf Code
//! zeigen, und der zieht egui, winit und den Rest mit — dieses Binary ist
//! damit so schwer wie die übrigen GUI-Belege, und die Decke misst etwas.

use std::path::Path;

/// Zwingt den Linker, den Einstieg der Oberfläche samt egui mitzunehmen —
/// sonst misst [`das_eigene_binary_traegt_keine_volle_debug_info`] ein
/// Binary, das nichts von dem trägt, worum es geht. Nur unter Linux, wie
/// sein einziger Aufrufer; sonst meldet der Windows-Clippy ihn als tot.
#[cfg(target_os = "linux")]
fn egui_ist_gelinkt() {
    let einstieg = redact_gui::run as fn(_) -> _;
    std::hint::black_box(einstieg);
}

/// Decke für die Dateigröße des eigenen Testbinaries, in MB (1024²).
/// Gemessen an diesem Binary: 111 MB mit Zeilentabellen, 272 MB ohne den
/// Knopf. (`zm_b_gleichzeitige_exporte` daneben: 78,0 MB, vorher 240,7 MB.)
pub const DECKE_MB: u64 = 180;

#[test]
fn die_stufe_steht_im_manifest() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("Workspace-Cargo.toml");
    let von = text
        .find("[profile.dev]")
        .expect("die Workspace-Cargo.toml hat keinen [profile.dev]-Block mehr");
    let block = &text[von + "[profile.dev]".len()..];
    let bis = block.find("\n[").unwrap_or(block.len());
    let block = &block[..bis];
    let zeile = block
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("debug"))
        .expect("im [profile.dev]-Block steht keine debug-Zeile");
    assert_eq!(
        zeile.replace(' ', ""),
        "debug=\"line-tables-only\"",
        "die Debug-Stufe des dev-Profils ist nicht mehr line-tables-only — dann trägt \
         jedes Testbinary wieder die volle Debug-Information aller Abhängigkeiten, \
         und das Gate passt nicht mehr auf die Platte"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn das_eigene_binary_traegt_keine_volle_debug_info() {
    egui_ist_gelinkt();
    let exe = std::env::current_exe().expect("eigenes Binary");
    let bytes = std::fs::metadata(&exe).expect("Größe").len();
    let mb = bytes / (1024 * 1024);
    eprintln!("{}: {} MB", exe.display(), mb);
    assert!(
        mb < DECKE_MB,
        "{} ist {mb} MB groß — über der Decke von {DECKE_MB} MB. Mit \
         `[profile.dev] debug = \"line-tables-only\"` waren es 111 MB, mit voller \
         Debug-Information 272 MB.",
        exe.display()
    );
}
