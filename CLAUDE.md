# Repo-Regeln — Nexo

## Zuerst lesen

[`docs/CONTEXT.md`](docs/CONTEXT.md) ist der Einstieg — die Karte des
Repositories: was wo liegt, welche Datei welche Frage beantwortet, und welche
Invarianten nicht gebrochen werden dürfen. Die Tabelle *Task → where* nennt für
die übliche Aufgabe die zwei bis drei Dateien, die man tatsächlich öffnen muss.

`docs/` umfasst gut 235 KB Prosa. Alles davon zu lesen, um einen Handler zu
ändern, ist der teure Fehler, den diese Karte verhindert.

## Account-Identität

Alles in diesem Repository läuft unter den Accounts der Menschen, die daran
schreiben — standardmässig **bananaaboy**. Es gibt keinen Assistenten-Account,
keine Werkzeug-Identität und keine Ko-Autorenschaft-Zeile für ein Werkzeug.

Konkret heisst das: in Commits, Commit-Messages, Pull Requests (Titel und Body),
Issues, Reviews, Review-Kommentaren, Code-Kommentaren, Doku und Changelogs
tauchen **keine** der folgenden Dinge auf:

- `Co-Authored-By:`-Zeilen — egal welcher Name
- `Claude-Session:` / `Assisted-By:` / `Generated-By:`-Zeilen
- "Generated with ..."-Fusszeilen und die dazugehörigen Links
- 🤖-Zeilen oder Emoji-Fusszeilen dieser Art
- Nennungen von Assistenten- oder Agent-Tools als Urheber

Branch-, Commit- und PR-Namen beschreiben die Änderung — nicht das Werkzeug,
mit dem sie entstanden ist.

Commit-Messages beschreiben nur die Code-Änderung. Nichts über Attribution,
nichts über den Entstehungsweg.

## Mitwirkende

Zwei Personen schreiben Code hier: **bananaaboy** und **YungDice**.

- Standard-Autor jedes Commits ist **bananaaboy** — das ist, was die lokale
  Git-Config unten setzt.
- Macht **YungDice** einen Commit, läuft der unter seinem eigenen Account. Was
  nie passiert: ein Commit unter einer dritten, nicht-menschlichen Identität.

Urheberrechtlich ist das Miturheberschaft (Art. 7 URG) — beide müssen einer
Lizenzänderung zustimmen. Die Copyright-Zeile im [`LICENSE`](LICENSE) nennt
**`delidev`**: ein Name über beiden Personen, keine juristische Person und
selbst kein Rechtsträger. Was daraus folgt — wer unterschreiben kann, was
schriftlich festgehalten gehört, und warum eine Umbenennung der Zeile keine
Lizenzänderung ist — steht in [`docs/LICENSING.md`](docs/LICENSING.md) §1.

Upstream ist <https://github.com/YungDice/nexo>.

## Sparsam arbeiten

Kontext ist die knappe Ressource. Die Gewohnheiten, die eine Sitzung brauchbar
halten — ausführlicher in [`docs/CONTEXT.md`](docs/CONTEXT.md#working-economically):

1. **Erst routen, dann lesen.** *Task → where* in `docs/CONTEXT.md` nennt die
   richtigen Dateien. `crates/store/src/lib.rs` ganz zu öffnen kostet rund
   25 000 Token; ein gezieltes `grep -n "TABLE IF NOT EXISTS messages" -A 20` kostet
   fast nichts und beantwortet meistens die Frage.
2. **Nach dem Symbol greppen, dann den Bereich lesen** — `sed -n '400,460p'`.
   Ganze Dateien nur, wenn sich ihre Struktur ändert.
3. **`BRIEF.md` und `RESEARCH-COMPARISON.md` sind Nachschlagewerke**, zusammen
   67 KB. Wird "brief §4.3" zitiert: `grep -n "^### 4.3" docs/BRIEF.md`, dann
   den Abschnitt lesen. Nie am Stück.
4. **Modul-Header sind verlässlich.** Jedes `lib.rs` beginnt mit einem
   Doc-Kommentar, der sagt, was die Crate darf und was nicht. Vierzehn Zeilen
   statt der ganzen Crate.
5. **Build eng ziehen.** `cargo test -p nexo-client` statt `--workspace`,
   `cargo clippy -p <crate>` statt über alles.
6. **`docs/STATUS.md`, bevor ein Feature als fehlend gilt.** Es wurde am Code
   entlang geschrieben, nicht an den Commit-Messages.
7. **Nichts neu herleiten, was schon entschieden ist.** Wirkt eine Entscheidung
   seltsam, steht der Grund geschrieben — meist in `RESEARCH-COMPARISON.md` oder
   im Kommentar an Ort und Stelle. Suchen statt neu aufrollen.
8. **`.\scripts\check.ps1` vor dem Push**, statt zu raten, was CI will. Ein
   geprüfter Push ist besser als drei spekulative.

## Setup für einen neuen Clone

Nach jedem frischen Clone einmal ausführen:

```sh
git config user.name  "bananaaboy"
git config user.email "116681483+bananaaboy@users.noreply.github.com"
git config core.hooksPath .githooks
```

Der dritte Befehl aktiviert `.githooks/commit-msg`. Der Hook entfernt die oben
genannten Zeilen automatisch aus jeder Commit-Message — als Netz, nicht als
Ersatz für die Regel.
