#!/usr/bin/env python3
"""Create three reproducible CSV datasets from OSM addresses with system PyQGIS.

Input is a Geofabrik Germany PBF. Downloads are cached; OSM usernames and other
personal metadata are neither requested nor exported. No address text is retained.
"""
import argparse
import csv
import gzip
import json
import os
from pathlib import Path
import subprocess
import sys
import urllib.request

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from qgis.core import QgsGeometry, QgsJsonUtils, QgsPoint, QgsPointXY

ROOT = Path(__file__).resolve().parents[1]


def download(url, path):
    if not path.exists():
        request = urllib.request.Request(url, headers={"User-Agent": "vibegis-data-preparation/0.1"})
        with urllib.request.urlopen(request) as response, path.with_suffix(path.suffix + ".part").open("wb") as out:
            while block := response.read(1024 * 1024):
                out.write(block)
        path.with_suffix(path.suffix + ".part").replace(path)


def poly_geometry(path):
    current, hole = [], False
    polygons = []
    for line in path.read_text().splitlines()[1:]:
        text = line.strip()
        if not text:
            continue
        if text == "END":
            if current:
                if current[-1] != current[0]:
                    current.append(current[0])
                if hole and polygons:
                    polygons[-1].append(current)
                else:
                    polygons.append([current])
                current = []
            continue
        values = text.split()
        if len(values) == 2:
            current.append(QgsPointXY(float(values[0]), float(values[1])))
        else:
            hole = text.startswith("!")
    return QgsGeometry.fromMultiPolygonXY(polygons)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=ROOT / ".cache/germany.osm.pbf")
    parser.add_argument("--output", type=Path, default=ROOT / "assets/data")
    args = parser.parse_args()
    cache = ROOT / ".cache"
    cache.mkdir(exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=True)
    download("https://download.geofabrik.de/europe/germany-latest.osm.pbf", args.input)
    for key, url in [("deutschland", "https://download.geofabrik.de/europe/germany.poly"),
                     ("bayern", "https://download.geofabrik.de/europe/germany/bayern.poly")]:
        download(url, args.output / f"{key}.poly")
    boundary = cache / "regensburg-boundary.json"
    download("https://nominatim.openstreetmap.org/search?city=Regensburg&country=Germany&format=jsonv2&polygon_geojson=1&limit=1", boundary)
    regensburg = json.loads(boundary.read_text())[0]
    if regensburg["osm_type"] != "relation" or regensburg["osm_id"] != 62411:
        raise RuntimeError("Unexpected Regensburg boundary; inspect before continuing")
    geometries = {key: poly_geometry(args.output / f"{key}.poly") for key in ["deutschland", "bayern"]}
    geometries["regensburg"] = QgsJsonUtils.geometryFromGeoJson(json.dumps(regensburg["geojson"]))
    engines = {}
    boxes = {}
    for key, geometry in geometries.items():
        engine = QgsGeometry.createGeometryEngine(geometry.constGet())
        engine.prepareGeometry()
        engines[key] = engine
        boxes[key] = geometry.boundingBox()

    filtered = cache / "addresses.osm.pbf"
    if not filtered.exists():
        temp = cache / "addresses.part.osm.pbf"
        subprocess.run(["osmium", "tags-filter", str(args.input), "nwr/addr:housenumber", "-o", str(temp), "--overwrite", "--no-progress"], check=True)
        temp.replace(filtered)
    # Preserve OSM element IDs, not osmium's area IDs. Only point and area
    # geometries are emitted, so closed ways cannot appear twice as lines/areas.
    export = subprocess.Popen(["osmium", "export", str(filtered), "-f", "geojsonseq", "--attributes=type,id",
                               "--geometry-types=point,polygon", "-i", "sparse_file_array," + str(cache / "locations.idx"),
                               "--no-progress"], stdout=subprocess.PIPE, text=True, encoding="utf-8")
    files = {key: gzip.open(args.output / f"{key}.csv.gz.part", "wt", encoding="utf-8", newline="", compresslevel=1) for key in geometries}
    writers = {key: csv.writer(file, lineterminator="\n") for key, file in files.items()}
    counts = dict.fromkeys(geometries, 0)
    skipped = 0
    for writer in writers.values():
        writer.writerow(["id", "status", "longitude", "latitude"])
    try:
        for line in export.stdout:
            feature = json.loads(line.lstrip("\x1e"))
            properties = feature["properties"]
            if "addr:housenumber" not in properties:
                continue
            geometry = feature["geometry"]
            if geometry["type"] == "Point":
                x, y = geometry["coordinates"][:2]
            else:
                point = QgsJsonUtils.geometryFromGeoJson(json.dumps(geometry, separators=(",", ":"))).pointOnSurface()
                if point.isEmpty():
                    skipped += 1
                    continue
                x, y = point.asPoint()
            qpoint = QgsPoint(x, y)
            if not boxes["deutschland"].contains(x, y) or not engines["deutschland"].intersects(qpoint):
                continue
            kind = properties["@type"][0]
            osm_id = int(properties["@id"])
            status = ["rot", "gelb", "gruen"][(osm_id * 2654435761 + ord(kind)) % 3]
            row = [f"{kind}{osm_id}", status, f"{x:.7f}", f"{y:.7f}"]
            for key, writer in writers.items():
                if key == "deutschland" or (boxes[key].contains(x, y) and engines[key].intersects(qpoint)):
                    writer.writerow(row)
                    counts[key] += 1
            if counts["deutschland"] % 100_000 == 0:
                print(json.dumps(counts), flush=True)
        if export.wait() != 0:
            raise RuntimeError("osmium export failed; incomplete output was not published")
    finally:
        for file in files.values():
            file.close()
    if not all(counts.values()):
        raise RuntimeError(f"An empty dataset was generated: {counts}")
    datasets = []
    settings = {
        "regensburg": ("Regensburg", "OSM-Adressobjekte innerhalb der Stadtgrenze.", [12.0969, 49.0134], 14),
        "bayern": ("Bayern", "Gesamter Bayern-Ausschnitt. Größerer Speicherbedarf.", [11.5, 48.9], 7),
        "deutschland": ("Deutschland", "Gesamter Deutschland-Ausschnitt. Für Rechner mit ausreichend RAM.", [10.45, 51.15], 6),
    }
    for key, (name, description, center, zoom) in settings.items():
        path = args.output / f"{key}.csv.gz"
        path.with_suffix(path.suffix + ".part").replace(path)
        datasets.append(dict(key=key, name=name, description=description, center=center, zoom=zoom, rows=counts[key], bytes=path.stat().st_size))
    info = subprocess.check_output(["osmium", "fileinfo", "-j", str(args.input)], text=True)
    manifest = dict(datasets=datasets, source="OpenStreetMap / Geofabrik", license="ODbL-1.0", attribution="© OpenStreetMap contributors",
                    source_header=json.loads(info), skipped_empty_geometry=skipped,
                    method="addr:housenumber nodes and areas; area pointOnSurface; clip to Geofabrik polygons / Regensburg OSM relation 62411; preserve separate OSM objects")
    path = args.output / "manifest.json"
    path.with_suffix(".json.part").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    path.with_suffix(".json.part").replace(path)
    print("Finished:", counts, flush=True)


if __name__ == "__main__":
    main()
