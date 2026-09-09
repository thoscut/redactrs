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
//! ## Was auf welchem System wirklich passiert
//!
//! | System | Mittel | Wie fest |
//! |---|---|---|
//! | Linux | `prctl(PR_SET_DUMPABLE, 0)` | Der Kernel schreibt gar nichts, `root` eingeschlossen. |
//! | macOS, BSD, übrige Unix | `setrlimit(RLIMIT_CORE, 0)` | Schwächer: eine Grenze, kein Verbot. Wer den Prozess mit angehobener Grenze startet, ändert daran nichts — sie wird hier gesetzt, nicht geerbt —, aber ein `core_pattern`, das an ein Programm weiterreicht, kann sie je nach System übergehen. |
//! | Windows | **keins** | Ein Prozess kann sich dem Abbild nicht entziehen; `MiniDumpWriteDump` liegt beim Aufrufer, nicht beim Ziel. |
//!
//! Die Windows-Zeile ist die unangenehme: dort sitzt die Zielgruppe dieses
//! Werkzeugs. Deshalb sagt [`CoreDumps::Unavailable`] das auch aus, statt
//! Erfolg zu melden — eine Funktion, die auf drei Systemen `true` liefert und
//! nur auf einem etwas tut, wäre genau die Sorte Zusage, gegen die dieses
//! Projekt arbeitet. Das Restrisiko steht ausgeschrieben in `SECURITY.md`.
//!
//! ## Die eine Ausnahme von `unsafe`
//!
//! Sieben der acht Crates dieses Projekts stehen unter
//! `#![forbid(unsafe_code)]`. Genau eine Datei im ganzen Baum braucht es
//! nicht: diese, für den Systemaufruf unten. `redact-cli` steht deshalb unter
//! `deny` statt `forbid`, und die Ausnahme trägt der einzelne Aufruf — nicht
//! die Datei und nicht das Crate.
//!
//! Wer das nachzählen will:
//!
//! ```console
//! $ grep -rl 'forbid(unsafe_code)' crates/*/src/lib.rs crates/*/src/main.rs | wc -l
//! $ grep -rl '#\[allow(unsafe_code)\]' crates/*/src --include='*.rs'
//! $ grep -c '#\[allow(unsafe_code)\]' crates/redact-cli/src/dumpable.rs
//! ```
//!
//! Die erste Zahl ist **8**, nicht 7: `redact-gui` hat zwei Wurzeln (`lib.rs`
//! und `main.rs`), beide unter `forbid`. Gezählt werden dort Dateien, hier im
//! Text Crates — der Unterschied ist genau diese eine Doppelung.
//!
//! Die zweite Liste nennt **nur diese Datei**, und die dritte Zahl ist **3**:
//! `prctl` (Linux), `setrlimit` (übrige Unix) — je Bau entsteht nur einer von
//! beiden — und die Gegenprobe im Test. Gezählt wird das Attribut, nicht das
//! Schlüsselwort: ein `grep` nach `unsafe {` träfe auch diesen Kommentar.

/// Was der Versuch ergeben hat, Kernabzüge abzuschalten.
///
/// Drei Fälle statt `bool`, weil „ging nicht" und „gibt es hier nicht" zwei
/// verschiedene Nachrichten sind: die eine ist ein Fehler, den man melden
/// kann, die andere eine Eigenschaft des Systems, an der niemand etwas
/// ändert. Sie unter `false` zusammenzufassen hieße, auf Windows bei jedem
/// Lauf eine Warnung zu drucken, die niemand befolgen kann — und unter `true`
/// hieße es, Schutz zu behaupten, den es dort nicht gibt.
///
/// Auf einem System ohne Mittel (Windows) liefert [`deny_core_dumps`] nur
/// [`CoreDumps::Unavailable`]; `Disabled` und `Failed` werden dort nie gebaut,
/// und `clippy -D warnings` hielte das für toten Code. Die beiden Zustände
/// sind aber die Wahrheit über Linux und die übrigen Unix — ein Enum je System
/// wäre die schlechtere Ehrlichkeit. Deshalb die Ausnahme, gebunden an genau
/// die Systeme, auf denen sie zutrifft.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreDumps {
    /// Abgeschaltet. Der Klartext des Dokuments landet bei einem Absturz
    /// nicht auf der Platte.
    Disabled,
    /// Dieses System bietet kein Mittel dagegen. Zwei Fälle: Windows kennt
    /// von vornherein keines, und ein Unix-Kern kann die Option abgelehnt
    /// haben (`EINVAL`/`ENOSYS` — etwa unter einem Filter, der `prctl`
    /// beschneidet).
    ///
    /// Getrennt von [`CoreDumps::Failed`], weil es zwei verschiedene
    /// Nachrichten sind: hier ist nichts kaputt, es gibt nur nichts zu
    /// holen. Eine Warnung wäre an dieser Stelle ein Ratschlag, den niemand
    /// befolgen kann.
    Unavailable,
    /// Das Mittel gibt es, der Aufruf schlug fehl. Der Schutz ist **nicht**
    /// aktiv, und das gehört gesagt.
    Failed,
}

/// Deutet den Rückgabewert eines der beiden Systemaufrufe.
///
/// `EINVAL` und `ENOSYS` heißen „diesen Aufruf gibt es hier nicht" — ein Kern
/// ohne die Option, oder ein Filter (seccomp, gehärtete Container), der sie
/// abschneidet. Das ist [`CoreDumps::Unavailable`] und keine Störung. Jeder
/// andere Fehler ist einer.
///
/// Der Fehlercode kommt über [`std::io::Error::last_os_error`] — die sichere
/// Fassung; `errno` selbst zu lesen bräuchte ein zweites `unsafe`, für
/// dieselbe Zahl.
#[cfg(unix)]
fn outcome(rc: libc::c_int) -> CoreDumps {
    if rc == 0 {
        return CoreDumps::Disabled;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EINVAL) | Some(libc::ENOSYS) => CoreDumps::Unavailable,
        _ => CoreDumps::Failed,
    }
}

/// Verbietet dem Kernel, von diesem Prozess einen Abzug zu schreiben.
///
/// Was auf welchem System geschieht, steht in der Tabelle im Modulkommentar.
/// Ein Aufruf genügt, und er gehört an den Anfang von `main`: alles davor
/// liefe ungeschützt.
pub fn deny_core_dumps() -> CoreDumps {
    #[cfg(target_os = "linux")]
    {
        // Eine der drei `unsafe`-Stellen im ganzen Baum (die anderen: der
        // `setrlimit`-Zweig darunter und die Gegenprobe im Test).
        //
        // `prctl` ist variadisch; die Argumente nach `PR_SET_DUMPABLE` sind
        // für diese Option als 0 vorgeschrieben (`man 2 prctl`). Der Aufruf
        // liest und schreibt keinen Speicher, hat keine Vorbedingung außer
        // gültigen Konstanten und kann den Prozesszustand nur in die eine
        // Richtung ändern, die hier gewollt ist.
        #[allow(unsafe_code)]
        let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
        outcome(rc)
    }

    // macOS, BSD und übrige Unix: `prctl` gibt es dort nicht, `RLIMIT_CORE`
    // schon. Das ist das schwächere Mittel (eine Grenze, kein Verbot), aber
    // es ist eines — und „gar nichts tun und `true` melden" wäre die
    // schlechteste der drei Möglichkeiten.
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        #[allow(unsafe_code)]
        let rc = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) };
        outcome(rc)
    }

    #[cfg(not(unix))]
    {
        CoreDumps::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Aufruf liefert je Ziel, was die Funktion dort ehrlich liefern
    /// **kann** — und unter Linux ist er danach **nachweisbar** wirksam.
    ///
    /// Die Rückgabe allein wäre kein Beleg: eine Funktion kann `Disabled`
    /// melden, ohne etwas getan zu haben. Unter Linux wird deshalb der
    /// Zustand gegengelesen, den der Kernel selbst führt.
    ///
    /// Dieselbe Dreiteilung wie in `main.rs`, und aus demselben Grund:
    ///
    /// * **Linux** streng `Disabled` — `prctl` gibt es dort immer, und die
    ///   Gegenprobe darunter fragt den Kernel.
    /// * **übrige Unix** nur `!= Failed` — `setrlimit` kann unter einem
    ///   Syscall-Filter ehrlich `Unavailable` melden, und das ist kein
    ///   Fehler des Programms.
    /// * **nicht Unix** (Windows) genau `Unavailable` — dort gibt es kein
    ///   Mittel, und die Funktion darf das nicht als Erfolg ausgeben.
    ///
    /// Eine frühere Fassung verlangte überall `Disabled`; der Windows-Job
    /// der CI war damit rot, obwohl die Funktion dort genau das tat, was
    /// der Modulkommentar zusagt.
    #[test]
    fn the_process_is_no_longer_dumpable() {
        let result = deny_core_dumps();

        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                result,
                CoreDumps::Disabled,
                "Kernabzüge liessen sich nicht abschalten"
            );
            // `/proc/self/status` führt den Zustand als `CoreDumping`? Nein —
            // die Zeile heißt seit jeher anders, deshalb wird der Kernel
            // direkt gefragt.
            #[allow(unsafe_code)]
            let state = unsafe { libc::prctl(libc::PR_GET_DUMPABLE) };
            assert_eq!(state, 0, "der Prozess ist weiterhin abzugsfähig");
        }

        #[cfg(all(unix, not(target_os = "linux")))]
        assert_ne!(
            result,
            CoreDumps::Failed,
            "setrlimit(RLIMIT_CORE, 0) schlug fehl — der Schutz ist nicht aktiv"
        );

        #[cfg(not(unix))]
        assert_eq!(
            result,
            CoreDumps::Unavailable,
            "auf einem System ohne Mittel darf die Funktion keinen Schutz behaupten"
        );
    }
}
