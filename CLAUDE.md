# Repo-Regeln — Nexo

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
Lizenzänderung zustimmen. Die Details stehen in
[`docs/LICENSING.md`](docs/LICENSING.md); die Copyright-Zeile im `LICENSE`
nennt `filiusfetish`.

Upstream ist <https://github.com/YungDice/nexo>.

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
