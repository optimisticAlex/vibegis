# vibegis

Ein GIS-MVP im Browser: Rust + WASM, egui, OpenStreetMap und ein räumlich
indizierter Arbeitsspeicher. Hauspunkte laden, ein Polygon zeichnen und den Status
aller Treffer ändern. Ein Backend oder Browser-Persistenz ist nicht erforderlich.

## Starten

Voraussetzungen: Rust über rustup, das WASM-Target und Trunk. Die Toolchain wird
über `rust-toolchain.toml` festgelegt; `Cargo.lock` fixiert die Abhängigkeiten.

```bash
source "$HOME/.cargo/env"
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.22.0 --locked
./scripts/dev.sh
```

Öffne **http://127.0.0.1:8787**. Der Entwicklungsmodus startet Trunk sowie einen
lokalen Datenserver auf Port 8788. Bereits vorbereitete Daten liegen unter
`assets/data/`; zum Neuaufbau siehe unten.

Für den optimierten statischen Build:

```bash
trunk build --release
python3 scripts/serve.py
```

Der Python-Server liefert `dist/` und die CSV-Dateien aus. Er enthält keine
Anwendungslogik und speichert keine Änderungen. Für andere Rechner im lokalen
Netz kann explizit `--host 0.0.0.0` gesetzt werden.

## Bedienung

- **CSV Daten laden:** Regensburg, Bayern, Deutschland oder eine eigene Datei.
  Regensburg wird beim Start automatisch geladen, sofern die Demodaten vorliegen.
- Ziehen verschiebt die Karte, das Mausrad zoomt. `+`, `−` und die Home-Taste am
  Kartenrand steuern Zoom und Rückkehr zum Datensatz.
- **Polygon auswählen:** Eckpunkte anklicken, mit Enter, Doppelklick oder
  „Auswahl abschließen“ beenden. Backspace entfernt den letzten Eckpunkt; Esc
  bricht die Auswahl ab. Die Karte lässt sich während des Zeichnens weiter zoomen.
- Rot, Gelb oder Grün setzt den Status aller ausgewählten Punkte. Polygonränder
  zählen mit. Selbstschneidende und flächenlose Polygone werden zurückgewiesen.
- Die letzte Statusänderung lässt sich zurücknehmen.
- **CSV exportieren** sichert den gesamten geladenen Datensatz einschließlich
  der Änderungen. Die Datei kann über denselben Import wieder eingelesen werden.

Es gibt keine automatische Speicherung über Browsersitzungen hinweg. Ein neuer
Import ersetzt den aktuellen Stand erst nach erfolgreicher Prüfung. Während
Import, Auswahl, Bearbeitung und Export sind konkurrierende Änderungen gesperrt.

In der Übersicht (Zoom < 12) werden Punkte in 0,05°-Zellen nach Status verdichtet
dargestellt; ab Zoom 12 werden alle Punkte im Kartenausschnitt einzeln gezeichnet.
Die Polygonauswahl arbeitet immer mit den Originalpunkten, unabhängig vom Zoom.

## CSV-Format

UTF-8, Komma als Trennzeichen, exakt diese Spaltenreihenfolge:

```csv
id,status,longitude,latitude
n123,rot,12.0969000,49.0134000
w456,gelb,12.1020000,49.0150000
r789,gruen,12.1100000,49.0120000
```

- `id`: eindeutige nichtnegative Zahl < 2^62; optional `n`, `w` oder `r` als
  Präfix für OSM Node, Way und Relation. Die Präfixe sind Teil der Identität.
- `status`: `rot`, `gelb`, `gruen` (auch `grün` und englische Farbnamen beim Import).
- `longitude`, `latitude`: Punktgeometrie in WGS84, Dezimalpunkt. Unterstützter
  Bereich: −180 bis 180° Länge, −85 bis 85° Breite für die Web-Mercator-Karte.
- Doppelte IDs, ungültige Statuswerte, nichtendliche Koordinaten und falsche
  Spalten erzeugen einen Fehler. Es werden keine ungültigen Zeilen still verworfen.

## OSM-Demodaten vorbereiten

Systempakete für die Aufbereitung unter Ubuntu:

```bash
sudo apt install qgis python3-qgis gdal-bin osmium-tool
python3 scripts/prepare_data.py
```

Das Skript nutzt das System-Python mit PyQGIS (QGIS >= 3.36). Es lädt den
Deutschland-PBF von [Geofabrik](https://download.geofabrik.de/europe/germany.html),
filtert `addr:housenumber` und erzeugt:

- `assets/data/regensburg.csv.gz`: innerhalb der Stadtgrenze, OSM-Relation 62411.
- `assets/data/bayern.csv.gz`: innerhalb des Geofabrik-Bayern-Ausschnitts.
- `assets/data/deutschland.csv.gz`: innerhalb des Geofabrik-Deutschland-Ausschnitts.
- `assets/data/manifest.json`: tatsächliche Punktzahlen, Dateigrößen, Quelle,
  Datenstand des PBF und Verarbeitungshinweise.

Der vorbereitete Datenstand vom **18.10.2026** enthält:

| Datensatz | Punkte | Komprimierter Download |
| --- | ---: | ---: |
| Regensburg | 27.305 | 0,36 MB |
| Bayern | 2.672.296 | 36,71 MB |
| Deutschland | 20.776.482 | 282,91 MB |

Nodes behalten ihre Position. Aus adressierten Flächen wird mit PyQGIS ein
innerhalb der Fläche liegender Punkt (`pointOnSurface`) erzeugt. Lineare Objekte
und nicht rekonstruierbare Flächen werden vom Osmium-Geometrieexport ausgelassen.
Eigenständige OSM-Objekte bleiben eigenständig; identische reale Adressen können
mehrfach erfasst sein. OSM liefert keinen vollständigen amtlichen Hausbestand.

Die Statusfarben werden deterministisch aus der OSM-ID erzeugt und haben keine
fachliche Bedeutung. Adresstexte und OSM-Benutzermetadaten werden nicht exportiert.
Quelldateien werden unter `.cache/` wiederverwendet. Für einen neueren Datenstand
die Eingabedatei **und** `.cache/addresses.osm.pbf` vor einem erneuten Lauf ersetzen.
Der erste Lauf benötigt mehrere GB Download und freien Plattenplatz sowie Zeit
für die Geometrieaufbereitung. Große Dateien gehören nicht ins Git-Repository.

Der Server liefert die gzip-Dateien unter `/data/<name>.csv` mit
`Content-Encoding: gzip`. Der Browser dekomprimiert sie beim Lesen. Beim Hosting
müssen dieselben Header und Pfade eingerichtet oder die CSVs dekomprimiert unter
diesen Pfaden abgelegt werden. Eigene Dateiimporte erwarten unkomprimierte CSVs.

## Entwicklung und Prüfung

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --target wasm32-unknown-unknown --lib -- -D warnings
cargo test
trunk build --release
cargo run --release --bin inspect_data -- assets/data/regensburg.csv.gz
# Optional: größere native Prüfung, begrenzt auf eine Million Punkte
cargo run --release --bin inspect_data -- assets/data/deutschland.csv.gz 1000000
```

`inspect_data` verwendet dasselbe Datenmodell wie die Browser-App und meldet
Punktzahlen, Statusverteilung, Importzeit und eine Polygonauswahl in Regensburg.
Native Messzeiten sind keine Browser-Benchmarks.

Bei der Entwicklung benötigten eine Million echte OSM-Punkte im nativen
Release-Build etwa 0,6 Sekunden Importzeit und 64 MiB Spitzenspeicher. Eine
Polygonauswahl in Regensburg dauerte etwa 0,11 ms. Der Zellindex wurde gegenüber
einem zuvor gemessenen R-Baum gewählt, der beim gleichen Import etwa 1,9 Sekunden
und 178 MiB benötigte. Das sind lokale Einzelmessungen, keine Leistungszusagen
für andere Browser, Geräte oder Datenverteilungen.

Automatisierte Firefox-Tests für Dateistreams, Duplikaterkennung, Abbruch und
den WebGL-Start mit Polygonauswahl und Statusänderung:

```bash
# wasm-bindgen-cli muss zur Version in Cargo.lock passen.
cargo install wasm-bindgen-cli --version 0.2.129 --locked
GECKODRIVER="$(command -v geckodriver)" \
  CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown --lib
```

Diese Tests benötigen Firefox und Geckodriver. Die visuelle Abnahme erfolgt
zusätzlich manuell: Karte und Attribution prüfen, Polygon zeichnen, Status ändern,
zurücknehmen, exportieren und die exportierte Datei erneut laden. Bei automatischen
UI-Tests keine OSM-Kacheln durch systematisches Verschieben oder Zoomen abrufen.
Bei Snap-Firefox kann `GECKODRIVER=/snap/firefox/current/usr/lib/firefox/geckodriver`
den direkten Treiber auswählen, falls der Snap-Starter nicht rechtzeitig startet.

## Struktur und Architektur

```text
src/model.rs             Datenmodell, räumlicher Zellindex, Auswahl, Status und CSV-Regeln
src/browser.rs           Browser-Streams, Dateiimport, Export und asynchrone Jobs
src/app.rs               egui-Oberfläche und Kartenebenen
src/bin/inspect_data.rs  Native Datenprüfung und Messungen
tests/model.rs          Fachliche Regressionstests
scripts/                Datenaufbereitung und lokale Server
assets/data/            CSV-Demodaten und Quellenmanifest
```

Laden, Auswahl und Export geben regelmäßig an den Browser zurück; Statusänderungen
werden in kleinen Blöcken verarbeitet. Die App hält den ausgewählten Datensatz
vollständig im RAM. Deutschland benötigt erheblich mehr Speicher als Regensburg;
Streaming vermeidet eine zusätzliche vollständige CSV-Kopie, ersetzt aber keinen
RAM für Punkte und Index. Beim Austausch bleibt der bisherige Datensatz bis zum
erfolgreichen Import im Speicher. Ein Tab-Neustart vor einem sehr großen Import
reduziert diesen zusätzlichen Speicherbedarf.

Als nächste Ausbaustufe kann eine Rust-HTTP-API Daten räumlich nachladen und
Statusänderungen über SQLx nach PostGIS schreiben. Authentifizierung,
Versionskonflikte und Mehrbenutzer-Synchronisation sind noch nicht implementiert.

## Daten- und Kartennachweise

Adressdaten: **© OpenStreetMap contributors**, [ODbL](https://www.openstreetmap.org/copyright).
Die Herkunft und Lizenz gelten auch für exportierte OSM-basierte Datensätze.
Hintergrund: `tile.openstreetmap.org`, interaktive Nutzung mit Browser-Caching,
sichtbarer Attribution und Referer gemäß [Tile Usage Policy](https://operations.osmfoundation.org/policies/tiles/).
Es gibt keinen Offline-Kacheldownload und keine Verfügbarkeitsgarantie des
öffentlichen Kartendienstes.
