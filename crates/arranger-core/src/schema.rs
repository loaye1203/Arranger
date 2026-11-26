pub fn describe() -> serde_json::Value {
    let error_codes = serde_json::json!(["INVALID_JSON","INVALID_REQUEST","PROTOCOL_VERSION","UNSUPPORTED_UNIT","INVALID_DOCUMENT","INVALID_PAGE","INVALID_OBJECT","INVALID_STYLE","INVALID_GEOMETRY","INVALID_PATH","INVALID_TEXT","INVALID_TEXT_LAYOUT","TEXT_AREA_OVERFLOW","NOT_TEXT","INVALID_TEXT_PATH","TEXT_RANGE","TEXT_LAYOUT_FAILED","TEXT_PATH_OVERFLOW","MISSING_GLYPH","UNSUPPORTED_GLYPH_OUTLINE","INVALID_RESOURCE","INVALID_CLIP","INVALID_POINT","INVALID_INDEX","INVALID_PARAMETER","NOT_A_PATH","UNSUPPORTED_GEOMETRY","OPEN_PATH","CROSS_PAGE","BOOLEAN_RANGE","BOOLEAN_FAILED","INVALID_LEASE","INVALID_DPI","INVALID_PAGES","PDF_PAGE_RANGE","DUPLICATE_ID","INVALID_PARENT","CYCLIC_GROUP","DOCUMENT_NOT_FOUND","DOCUMENT_EXISTS","OBJECT_NOT_FOUND","OBJECT_LOCKED","PAGE_NOT_FOUND","PAGE_NOT_EMPTY","RESOURCE_NOT_FOUND","RESOURCE_IN_USE","RESOURCE_ALREADY_EXISTS","DOCUMENT_BUSY","REVISION_CONFLICT","TRANSACTION_NOT_FOUND","EMPTY_BATCH","NOTHING_TO_UNDO","NOTHING_TO_REDO","UNSAVED_CHANGES","OUTPUT_EXISTS","DISK_CONFLICT","FILE_BUSY","IO_ERROR","CORRUPT_FILE","UNSUPPORTED_FILE_VERSION","CHECKSUM_MISMATCH","UNRECOVERABLE_FILE","CDR_SAVE_NOT_READY","CDR_IMPORT_NOT_READY","INVALID_CDR","CDR_UNSUPPORTED","CDR_REQUIRES_VERSION21","RENDER_SIZE_LIMIT","RENDER_ERROR","ENCODING_ERROR","SERIALIZATION_ERROR"]);
    serde_json::json!({
        "protocol_version": 1,
        "native_format_version": 5,
        "source_backup_recovery":{"command":"document_recover_backup","path":"absolute .arranger original or .arranger.bak; primary may be missing","result":"verified complete source, original saved revision, no file binding, dirty; original/backup/pending unchanged","versions":[1,2,3,4,5]},
        "native_format_versions_read": [1,2,3,4,5],
        "native_format_write": "v1 for RGB page-only documents, v2 for RGB documents containing _workspace, v3 when CMYK paints occur, v4 when fixed-world stroke flags occur, v5 when stroke_fixed_nib occurs; older readers reject newer versions before backup recovery",
        "unit": "mm",
        "workspace_capabilities":{"owner":"Object.page_id = _workspace; not a printable Page ID","placement":"object_place after geometry changes in the same batch: root source bounds touching the chosen page belong to it; completely outside belongs to workspace. Groups move as one subtree.","view":"page objects plus shared objects, including outside the paper; page navigation is read-only","output":"SVG/PNG/PDF include only objects owned by the chosen page, clipped to paper; workspace is excluded","coordinates":"same millimetre coordinates across active-page views; saved and undoable"},
        "object_types": ["rect", "ellipse", "path", "text", "image", "group"],
        "export_formats": ["svg", "png", "pdf", "cdr14", "cdr21"],
        "save_capabilities": {"query":"save_check","formats":["arranger","cdr14","cdr21"],"implemented":["arranger"],"cdr":"Complete source saves as .arranger. Explicit editable/appearance CDR delivery uses cdr_preflight and cdr_export(mode). This legacy save_check remains the strict full-native source-preservation query; source-field equality does not certify native layout."},
        "cdr_capabilities": {"status":"explicit editable/appearance delivery copies; complete source stays .arranger","import_command":"cdr_import","backup_recovery_command":"cdr_recover_backup","preflight_command":"cdr_preflight","document_export_command":"cdr_export","modes":["editable","appearance"],"mode_contract":{"editable":"Supported native text/objects stay editable; Corel font, spacing and fitted layout can differ. Unsupported combinations reject without dropping objects.","appearance":"Monochrome text is outlined in a derived copy using the current layout. Original source, history and binding are unchanged. Native curves remain editable; characters do not.","version":"CDR14 isolated group transparency rejects with CDR_REQUIRES_VERSION21; never silently changes version or removes opacity."},"document_export_formats":["cdr14","cdr21"],"default_document_export_format":"cdr14","import_versions":[1400,2100],"export_format":"cdr14","selected_page_export_formats":["cdr14","cdr21"],"version":1400,"container":"14 native ZIP/RIFF;21 genuine CDRM root and external data","scope":"ordered pages/workspace, supported native geometry/bitmap/text and explicit appearance outlines","unsupported":"unsupported effects, missing/non-monochrome glyphs, out-of-range geometry and unmeasured image profiles reject explicitly","storage":"complete source stays .arranger; bounded native parameter/resource notes are not a hidden document","compatibility":"mode-specific contract; representative native validation does not promise every third-party CDR","page_id":"optional explicit selected page; omitted keeps all pages and shared workspace","exchange_persistence":"preflight and native re-decode before writes; .pending/.bak atomic export, conflict and corrupt backup guards; transactions reject; does not rebind native Save","source_storage":"No full Arranger model or glyph-placement cache hidden in CDR. Original image containers and bounded native parameter notes remain supported.","legacy":"Omitting mode retains the existing experimental exchange; save_check remains the strict full-native capability query."},
        "pdf_capabilities": "Single-page export or pdf_export with all pages / explicit unique page IDs in requested order, physical size quantized on Skia's fixed 0.001 mm device grid, <= 5080 mm per side; vector shapes/clips/monochrome glyph outlines and embedded images. Color glyphs explicitly fall back to the shared page raster at requested DPI with PDF_RASTERIZED. RGB output; CMYK paints use an explicit unmanaged RGB approximation, no PDF/A, managed CMYK/separations or editable PDF import.",
        "group_duplication": "Subtree copy; descendants default to new_id.1, new_id.2 in source stacking order. Optional descendant_ids maps source IDs to explicit IDs. Internal text-path links remap, image originals remain shared; collisions/locks abort the whole transaction.",
        "image_capabilities": {
            "formats": ["image/png","image/jpeg"],
            "operations": ["image_import","resource_put","resource_delete"],
            "queries": ["resource_get"],
            "sharing": "image_import reuses identical original bytes by SHA-256 and reports RESOURCE_REUSED; objects retain separate IDs/placement/clip/style",
            "storage": "original encoded bytes once per native resource and once per SVG resource definition; instances reference the resource",
            "metadata": "resource_get and save_check report original PNG/JPEG ICC/EXIF/text payload offsets, encoded lengths and SHA256 plus JPEG component count; no interpretation/conversion, no copied metadata contents",
            "fit": ["contain","stretch","cover"],
            "limits": "64 MiB encoded, 64 million decoded pixels; PNG/JPEG raster bytes only, no EXIF autorotation or managed ICC/CMYK output"
        },
        "style_capabilities": {
            "paint": "none, #RRGGBB/#RRGGBBAA, cmyk(C,M,Y,K) integer 0..100 or cmyk255(C,M,Y,K) integer 0..255; preserved channels with explicit unmanaged RGB preview/export; independent paint alpha and object/group opacity",
            "stroke": "width_mm, linecap butt/round/square, linejoin miter/round/bevel, miterlimit >= 1; optional stroke_scale_with_object defaults true, false keeps vector stroke width fixed in page millimetres; optional stroke_fixed_nib with1..99 integer stretch_percent and signed i32 angle_millionth_degrees (0 vertical,90e6 horizontal) retains an elliptical page pen",
            "gradients": "not implemented; see docs/LIMITATIONS.md"
        },
        "clip_capabilities": {
            "scope": "object-local rect or closed path with nonzero/evenodd, applies to image/vector/text/group without altering source",
            "composition": "intersection with ancestor clips, legacy image clip_mm and cover viewport; empty clips hide completely",
            "queries": "object_get returns active_clips with owner, source and world_transform; bounds_mm remains source geometry/ink, without common clipping or stroke",
            "hit_test": "local and ancestor clips gate geometric hit testing; not per-image-alpha pixel picking",
            "booleans": "clipped operands rejected until local and ancestor clips are removed"
        },
        "text_capabilities": {
            "backend": "usvg 0.48.1 / harfrust / system fonts",
            "measurement": "point/area paragraphs, per-line source byte ranges, baselines, shaped advances, ink, glyph transforms and resolved fonts",
            "commands": ["text_measure"],
            "operations": ["text_set_path", "text_to_path"],
            "text_path": "one same-page path subpath, absolute offsets in text-parent millimetres; start/middle/end anchor",
            "text_path_projection": "fixed high-precision placed outlines shared by GUI, SVG, PNG and conversion; native text remains editable",
            "outline_conversion": "in-place path with stable ID; rejects missing, overflowing and non-monochrome outline glyphs",
            "paragraph_layout": "geometry.layout: point or area; left/center/right alignment, letter_spacing_mm, line_height_mm (default 1.2 em); object width/height set area",
            "wrapping": "basic Chinese/English word wrapping with UAX29 grapheme emergency breaks, common punctuation guards; no full UAX14 or hyphenation",
            "area_overflow": "all lines remain visible/editable; warning and measurement overflow=true; conversion refuses until full layout fits",
            "limits": "uniform horizontal text; no rich spans, justification, vertical alignment, font embedding or multi-line path text; see docs/LIMITATIONS.md"
        },
        "path_capabilities": {
            "coordinates": "absolute local mm; path_get indices include move_to and close",
            "elements": ["move_to", "line_to", "quad_to", "cubic_to", "close"],
            "fill_rules": ["nonzero", "evenodd"],
            "boolean_operations": ["union", "intersection", "difference", "xor"],
            "boolean_backend": "linesweeper 0.5.0 / ReallyCorrect",
            "boolean_tolerance_mm": "max(1e-6, max_abs_page_coordinate * 64 * f64::EPSILON); reject above 0.001",
            "boolean_inputs": "closed paths, rectangles, ellipses on one page; filled geometry without stroke",
            "boolean_result": "new root-level path in page coordinates, evenodd; inputs preserved unless remove_inputs=true"
        },
        "error_codes": error_codes,
        "schema": serde_json::from_str::<serde_json::Value>(include_str!("../../../docs/api-v1.schema.json")).expect("bundled schema is valid"),
        "scope": "V1 baseline on Windows: unified structured editing, paths/booleans, point/area text, shared PNG/JPEG, basic styles/clips, groups/pages, native recovery, SVG/PNG/PDF; macOS execution pending. See docs/LIMITATIONS.md"
    })
}
