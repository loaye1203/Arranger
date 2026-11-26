"""Generate a two-page vector and typography example for Arranger."""
import argparse
import json
from pathlib import Path
import sys


FONT = "PingFang SC" if sys.platform == "darwin" else "Microsoft YaHei"
GEOMETRY = "page-1"
TYPOGRAPHY = "typography"


def draw(object_id, page, x, y, width, height, geometry, fill, stroke="none", stroke_width=0):
    return {
        "type": "object_create",
        "object": {
            "id": object_id,
            "page_id": page,
            "x_mm": x,
            "y_mm": y,
            "width_mm": width,
            "height_mm": height,
            "geometry": geometry,
            "style": {"fill": fill, "stroke": stroke, "stroke_width_mm": stroke_width, "opacity": 1},
        },
    }


def text(object_id, page, x, y, content, size, color="#17334b"):
    geometry = {"kind": "text", "content": content, "font_family": FONT, "font_size_mm": size}
    return draw(object_id, page, x, y, 166, 10, geometry, color)


def operations():
    disk = {"kind": "ellipse"}
    return [
        {"type": "page_add", "page": {"id": TYPOGRAPHY, "width_mm": 210, "height_mm": 148}},
        text("geometry-title", GEOMETRY, 16, 12, "几何构图 / Geometric Study", 7),
        draw("night-sky", GEOMETRY, 16, 29, 178, 103, {"kind": "rect", "radius_mm": 7}, "#152b43"),
        draw("orbit", GEOMETRY, 39, 48, 120, 59, disk, "none", "#84a6bd", 0.65),
        draw("planet", GEOMETRY, 74, 49, 57, 57, disk, "#f1ad73"),
        draw("plains", GEOMETRY, 89, 61, 20, 12, disk, "#f7cba9"),
        draw("moon", GEOMETRY, 42, 83, 17, 17, disk, "#8ad9d1"),
        draw("small-star", GEOMETRY, 152, 44, 3, 3, disk, "#fff6d7"),
        {"type": "object_duplicate", "object_id": "small-star", "new_id": "distant-star"},
        {"type": "object_update", "object_id": "distant-star", "patch": {"x_mm": 166, "y_mm": 104}},
        text("type-title", TYPOGRAPHY, 16, 12, "文字与路径 / Type on a Path", 7),
        draw("circle-guide", TYPOGRAPHY, 62, 35, 86, 86, disk, "none", "#87adbe", 0.55),
        {"type": "object_to_path", "object_id": "circle-guide"},
        text("circle-text", TYPOGRAPHY, 0, 0, "中文排版 / Letterforms", 5),
        {"type": "text_set_path", "object_id": "circle-text",
         "text_path": {"path_id": "circle-guide", "start_offset_mm": 205, "baseline_offset_mm": -2, "anchor": "middle"}},
        text("type-center", TYPOGRAPHY, 88, 75, "Arranger", 7),
        text("type-caption", TYPOGRAPHY, 16, 130, "文字与路径均可编辑 / Editable text and path", 3.5),
    ]


def requests(output):
    commands = [
        {"type": "document_new", "document_id": "main", "width_mm": 210, "height_mm": 148, "unit": "mm"},
        {"type": "batch", "document_id": "main", "expected_revision": 0, "unit": "mm", "operations": operations()},
        {"type": "snapshot", "document_id": "main"},
        {"type": "text_measure", "document_id": "main", "object_id": "circle-text"},
        {"type": "save", "document_id": "main", "expected_revision": 1, "path": str(output / "vector-study.arranger")},
    ]
    for page, name in ((GEOMETRY, "geometry"), (TYPOGRAPHY, "typography")):
        for extension in ("svg", "png", "pdf"):
            commands.append({"type": "export", "document_id": "main", "page_id": page,
                             "format": extension, "path": str(output / f"{name}.{extension}")})
    return commands


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", type=Path, default=Path("artifacts/v1-example"))
    output = parser.parse_args().output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    path = output / "requests.jsonl"
    data = (json.dumps({"protocol_version": 1, "request_id": i, "command": command}, ensure_ascii=False)
            for i, command in enumerate(requests(output), 1))
    path.write_text("\n".join(data) + "\n", encoding="utf-8")
    print(path)


if __name__ == "__main__":
    main()

