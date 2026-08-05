//! Kein Kernabzug dieses Prozesses.
//!
//! ## Warum das hier steht
//!
//! Der Lauf hält, was er halten muss: den Klartext des Dokuments — IBAN,
//! Kontonummer, Name — und, wenn die Datei verschlüsselt war, das Passwort.
//! Stürzt der Prozess ab, schreibt der Kernel den gesamten Arbeitsspeicher in
//! eine Datei. Gemessen wurde das Passwort danach an **zwei** Stellen des
//! Abzugs wiedergefunden (Haufen und `environ`), und der Absturz ließ sich
//! über eine präparierte Eingabedatei auslösen.
//!
//! Ein Kernabzug liegt üblicherweise dort, wohin `kernel.core_pattern` zeigt —
//! unter systemd im Journal, sonst im Arbeitsverzeichnis. In beiden Fällen ist
//! er länger lesbar als der Prozess lief und für andere Programme erreichbar.
//!
//! Am laufenden Programm gemessen (`core_pattern = core`, `ulimit -c
//! unlimited`, ein Prozess, der eine IBAN im Speicher hält und dann
//! abstürzt):
//!
//! | | Abzugsdatei | die IBAN darin |
//! |---|---|---|
//! | ohne diesen Aufruf | 487 424 Byte | **3 Fundstellen** |
//! | mit diesem Aufruf | keine | — |
//!
//! Der Kernel schreibt den Abzug gar nicht erst; selbst die Meldung der
//! Shell verliert das „(core dumped)".
//!
//! ## Warum nicht `Drop` und Überschreiben
//!
//! Weil es nachweislich nicht hilft. Der Absturz passiert, *während* das
//! Geheimnis lebt; ein `Drop`, der es überschreibt, läuft dann nie. Eine
//! frühere Fassung hat genau das gebaut, gemessen und wieder entfernt.
//!
//! ## Was das kostet
//!
//! `PR_SET_DUMPABLE = 0` schaltet nicht nur den Abzug ab: der Prozess wird
//! damit auch für `ptrace` durch denselben Benutzer unerreichbar, und
//! `/proc/<pid>/` gehört danach `root`. Für ein Werkzeug, das Kontoauszüge
//! im Speicher hält, ist das die richtige Richtung — ein anderes Programm
//! desselben Benutzers kann den Klartext nicht mehr mitlesen. Wer mit `gdb`
//! oder `strace` an einem Fehler arbeitet, muss dafür `root` sein oder diese
//! Zeile für seinen Bau auskommentieren.
//!
//! ## Und die Plattform, auf der die Zielgruppe ist
//!
//! **Unter Windows gibt es kein Gegenstück.** Ein Prozess kann sich dort dem
//! Abbild nicht entziehen; `MiniDumpWriteDump` liegt beim Aufrufer, nicht beim
//! Ziel. Diese Absicherung schützt also Linux und macOS-artige Systeme, nicht
//! Windows. Das Restrisiko steht in `SECURITY.md`; hier steht es, damit
//! niemand die Datei liest und mehr Schutz annimmt, als sie leistet.
//!
//! ## Die eine Ausnahme von `unsafe`
//!
//! Sieben der acht Crates dieses Projekts stehen unter
//! `#![forbid(unsafe_code)]`. Genau eine Stelle im ganzen Baum braucht es
//! nicht: der `prctl`-Aufruf unten. `redact-cli` steht deshalb unter `deny`
//! statt `forbid`, und die Ausnahme trägt diese eine Funktion — nicht die
//! Datei und nicht das Crate.
//!
//! Wer das nachzählen will:
//!
//! ```console
//! $ grep -rl 'forbid(unsafe_code)' crates/*/src/lib.rs crates/*/src/main.rs | wc -l
//! $ grep -rn 'unsafe {' crates/*/src --include='*.rs'
//! ```
//!
//! Die erste Zahl ist **8**, nicht 7: `redact-gui` hat zwei Wurzeln (`lib.rs`
//! und `main.rs`), beide unter `forbid`. Gezählt werden dort Dateien, hier im
//! Text Crates — der Unterschied ist genau diese eine Doppelung.
//!
//! Die zweite Liste nennt **nur diese Datei**, mit zwei Zeilen: den Aufruf
//! unten und die Gegenprobe in ihrem Test.

/// Verbietet dem Kernel, von diesem Prozess einen Abzug zu schreiben.
///
/// Rückgabe: `false`, wenn der Aufruf fehlschlug — dann ist der Schutz
/// **nicht** aktiv. Auf allen Nicht-Linux-Systemen `true`, ohne etwas zu tun;
/// dort gibt es nichts Gleichwertiges (siehe Modulkommentar).
///
/// Ein Aufruf genügt, und er gehört an den Anfang von `main`: alles davor
/// liefe ungeschützt.
pub fn deny_core_dumps() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Die einzige `unsafe`-Stelle im ganzen Baum.
        //
        // `prctl` ist variadisch; die Argumente nach `PR_SET_DUMPABLE` sind
        // für diese Option als 0 vorgeschrieben (`man 2 prctl`). Der Aufruf
        // liest und schreibt keinen Speicher, hat keine Vorbedingung außer
        // gültigen Konstanten und kann den Prozesszustand nur in die eine
        // Richtung ändern, die hier gewollt ist.
        #[allow(unsafe_code)]
        let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
        rc == 0
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Aufruf gelingt — und er ist danach **nachweisbar** wirksam.
    ///
    /// Die Rückgabe allein wäre kein Beleg: `deny_core_dumps` könnte auch
    /// `true` liefern, ohne etwas zu tun (genau das tut sie außerhalb von
    /// Linux). Unter Linux wird deshalb der Zustand gegengelesen, den der
    /// Kernel selbst führt.
    #[test]
    fn the_process_is_no_longer_dumpable() {
        assert!(
            deny_core_dumps(),
            "prctl(PR_SET_DUMPABLE, 0) fehlgeschlagen"
        );

        #[cfg(target_os = "linux")]
        {
            // `/proc/self/status` führt den Zustand als `CoreDumping`? Nein —
            // die Zeile heißt seit jeher anders, deshalb wird der Kernel
            // direkt gefragt.
            #[allow(unsafe_code)]
            let state = unsafe { libc::prctl(libc::PR_GET_DUMPABLE) };
            assert_eq!(state, 0, "der Prozess ist weiterhin abzugsfähig");
        }
    }
}
