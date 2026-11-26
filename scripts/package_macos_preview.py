"""Build a local Apple Silicon preview without embedding build-machine paths."""
import argparse
import os
import platform
import plistlib
import re
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
HOME_PATH = Path.home().resolve()
BUILD_NUMBER = "371"
VERSION = "0.1.0"


def run(*args, env=None):
    subprocess.run(args, cwd=ROOT, env=env, check=True)


def deployment_target(binary):
    data = binary.read_bytes()
    magic, cpu = struct.unpack_from("<II", data)
    if (magic, cpu) != (0xFEEDFACF, 0x0100000C):
        raise ValueError("Expected a thin Apple Silicon executable")
    commands, size = struct.unpack_from("<II", data, 16)
    offset = 32
    versions = []
    for _ in range(commands):
        command, length = struct.unpack_from("<II", data, offset)
        if length < 8 or offset + length > 32 + size:
            raise ValueError("Invalid Mach-O load command")
        if command == 0x32 and length >= 24:
            target, version = struct.unpack_from("<II", data, offset + 8)
            if target == 1:
                versions.append(version)
        elif command == 0x24 and length >= 16:
            versions.append(struct.unpack_from("<I", data, offset + 8)[0])
        offset += length
    if not versions:
        raise ValueError("Missing macOS deployment target")
    version = max(11 << 16, *versions)
    return f"{version >> 16}.{(version >> 8) & 255}.{version & 255}"


def check_binary(binary):
    data = binary.read_bytes()
    home = re.compile(rb"/(?:Users|home)/[a-zA-Z][a-zA-Z0-9_.-]{1,30}/")
    if home.search(data) or os.fsencode(ROOT) in data or os.fsencode(HOME_PATH) in data:
        raise ValueError(f"Machine-local path remains in {binary.name}")


def build(output, offline):
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("Run on an Apple Silicon Mac with Xcode Command Line Tools")
    output.mkdir(parents=True, exist_ok=True)
    image = output / "Arranger-v0.1.0-preview.1-macos-arm64.dmg"
    if image.exists():
        raise FileExistsError(image)
    with tempfile.TemporaryDirectory(dir=output) as scratch:
        work = Path(scratch).resolve()
        target = work / "target"
        cargo_home = Path(os.environ.get("CARGO_HOME", HOME_PATH / ".cargo")).resolve()
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(target)
        flags = [
            f"--remap-path-prefix={HOME_PATH}=/build-home",
            f"--remap-path-prefix={ROOT}=/arranger",
            f"--remap-path-prefix={cargo_home}=/cargo",
            f"--remap-path-prefix={work}=/build",
            "-Cstrip=debuginfo",
        ]
        env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
        args = ["cargo", "build", "-p", "arranger", "--release", "--locked"]
        if offline:
            args.append("--offline")
        run(*args, env=env)
        binary = target / "release" / "arranger"
        check_binary(binary)

        stage = work / "image"
        app = stage / "Arranger.app"
        binaries = app / "Contents" / "MacOS"
        resources = app / "Contents" / "Resources"
        binaries.mkdir(parents=True)
        resources.mkdir()
        runtime = binaries / "arranger-bin"
        launcher = binaries / "Arranger"
        shutil.copy2(binary, runtime)
        run("/usr/bin/clang", "-Os", "-Wall", "-Wextra", "-Werror",
            "-mmacosx-version-min=11.0", str(ROOT / "scripts/macos_app_launcher.c"),
            "-o", str(launcher))
        check_binary(launcher)
        info = {
            "CFBundleName": "Arranger",
            "CFBundleDisplayName": "Arranger",
            "CFBundleIdentifier": "org.arranger.desktop",
            "CFBundleExecutable": "Arranger",
            "CFBundlePackageType": "APPL",
            "CFBundleVersion": BUILD_NUMBER,
            "CFBundleShortVersionString": VERSION,
            "LSMinimumSystemVersion": deployment_target(runtime),
            "NSHighResolutionCapable": True,
            "CFBundleDocumentTypes": [
                {"CFBundleTypeName": "Arranger document", "CFBundleTypeRole": "Editor",
                 "LSHandlerRank": "Owner", "LSItemContentTypes": ["org.arranger.document"]},
                {"CFBundleTypeName": "CorelDRAW document", "CFBundleTypeRole": "Editor",
                 "LSHandlerRank": "Alternate", "CFBundleTypeExtensions": ["cdr"]},
            ],
            "UTExportedTypeDeclarations": [
                {"UTTypeIdentifier": "org.arranger.document",
                 "UTTypeDescription": "Arranger document",
                 "UTTypeConformsTo": ["public.json"],
                 "UTTypeTagSpecification": {"public.filename-extension": ["arranger"]}},
            ],
        }
        (app / "Contents" / "Info.plist").write_bytes(plistlib.dumps(info))
        for name in ("LICENSE", "NOTICE"):
            shutil.copy2(ROOT / name, resources / name)
        notices = resources / "docs"
        notices.mkdir()
        for name in ("skia-native-notices.json", "third-party-aarch64-apple-darwin.json"):
            shutil.copy2(ROOT / "docs" / name, notices / name)
        shutil.copytree(ROOT / "docs/third-party-licenses", notices / "third-party-licenses")
        for executable in (runtime, launcher):
            run("/usr/bin/codesign", "--force", "--sign", "-", str(executable))
        run("/usr/bin/codesign", "--force", "--sign", "-", str(app))
        run("/usr/bin/codesign", "--verify", "--deep", "--strict", str(app))
        (stage / "Applications").symlink_to("/Applications", target_is_directory=True)
        (stage / "Read Me.txt").write_text(
            "Drag Arranger.app to Applications. This preview uses an ad-hoc signature "
            "and has not been notarized by Apple.\n", encoding="utf-8"
        )
        demo = work / "demo"
        run(sys.executable, str(ROOT / "examples/v1_design.py"), str(demo))
        run(str(runtime), "run", str(demo / "requests.jsonl"))
        samples = stage / "Samples"
        samples.mkdir()
        for name in ("vector-study.arranger", "geometry.pdf", "typography.pdf"):
            shutil.copy2(demo / name, samples / name)
        staged_image = work / image.name
        run("/usr/bin/hdiutil", "create", "-srcfolder", str(stage), "-volname", "Arranger",
            "-format", "UDZO", "-fs", "HFS+", str(staged_image))
        run("/usr/bin/hdiutil", "verify", str(staged_image))
        os.link(staged_image, image)
    print(image)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="Destination directory for the DMG")
    parser.add_argument("--offline", action="store_true", help="Use cached Cargo and Skia inputs")
    options = parser.parse_args()
    build(options.output.resolve(), options.offline)

