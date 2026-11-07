# Repository Guidelines

## Project Structure & Module Organization

vibegis is a Rust/WASM browser GIS MVP. `src/model.rs` contains point records, the spatial cell index, CSV validation, polygon selection, and status updates. `src/app.rs` implements the egui/walkers interface; `src/browser.rs` handles streaming imports, exports, and asynchronous browser jobs. `src/bin/inspect_data.rs` validates and benchmarks datasets natively.

Keep regression tests in `tests/`; browser tests live beside the relevant Rust modules. `scripts/` contains PyQGIS data preparation and local serving tools. Generated OSM CSVs belong in `assets/data/`; downloads and temporary outputs belong in ignored `.cache/`. Do not commit large datasets or build outputs.

## Build, Test, and Development Commands

- `./scripts/dev.sh`: start Trunk at port 8787 and the data server at 8788.
- `trunk build --release`: build the optimized browser application into `dist/`.
- `python3 scripts/serve.py`: serve the build and compressed datasets locally.
- `python3 scripts/prepare_data.py`: generate Regensburg, Bayern, and Germany CSVs using system PyQGIS and Osmium.
- `cargo test`: run native model regression tests.
- `cargo run --release --bin inspect_data -- FILE.csv.gz`: validate a dataset and measure import/selection performance.

See `README.md` for installation and Firefox/WASM test commands. Preserve the pinned toolchain and commit `Cargo.lock` when dependencies change.

## Coding Style & Naming Conventions

Use Rust 2024, four-space indentation, `snake_case` functions/modules, and `PascalCase` types. Run `cargo fmt --check` and Clippy for native and WASM targets with warnings denied. Keep browser-specific APIs behind WASM configuration. User-facing text is German.

## Testing Guidelines

Test CSV errors, duplicate IDs, spatial boundaries, invalid polygons, status changes, and export/import round trips. Browser tests must avoid requesting public OSM tiles; test builds disable the background layer. Distinguish native benchmarks, browser runtime tests, and visual verification in reports.

## Commit & Pull Request Guidelines

No historical commit convention is established. Use imperative subjects such as `Add streaming CSV import`. Describe behavior, validation, and relevant limitations; link issues and include screenshots for visible changes when available.

## Data & Configuration

Preserve OSM attribution and ODbL notices. Status colors are synthetic demo values. Never commit credentials. Keep changes in memory unless users explicitly export; imports must not replace valid data before validation succeeds.
