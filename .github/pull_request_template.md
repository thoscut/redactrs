<!--
Kurz halten. Diese Vorlage fragt nur nach dem, was hier tatsächlich
nachgeprüft wird — die ausführliche Fassung steht in CONTRIBUTING.md.
-->

## Was und warum

<!-- Was ändert sich am Verhalten, und welches Problem löst das? -->

## Der Beleg

<!--
Kein Befund ohne echten Lauf. Hier gehört die Ausgabe hin, die es zeigt —
der Befehl und was dabei herauskam, nicht "sollte jetzt gehen".

Geht es um Schwärzung: der Maßstab ist `redact_pdf::leaks` bzw.
`redact-rs <datei> --check-leaks "<text>"`. Nicht der eigene Extraktor,
und nicht `pdftotext | grep` — das gibt nachweislich falsche Entwarnung.
-->

```console

```

## Abhaken

- [ ] Das Gate läuft durch:
      `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
- [ ] Ein Test hält den Befund fest — und er wird **rot**, wenn die Änderung
      zurückgenommen wird (nachgeprüft, nicht angenommen).
- [ ] Verhaltensänderungen stehen in `CHANGELOG.md` unter „Unveröffentlicht“;
      geänderte Sicherheitszusagen mit **⚠ Sicherheit** gekennzeichnet.
- [ ] Keine echten personenbezogenen Daten — keine echte IBAN, kein echter
      Name, kein echter Kontoauszug, auch nicht in Testdaten.
- [ ] Neue Abhängigkeit? Dann steht unten, warum sie nötig ist und was sie
      an Angriffsfläche mitbringt. (Sonst diesen Punkt streichen.)
