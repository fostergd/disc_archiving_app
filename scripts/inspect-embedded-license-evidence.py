"""Review-only evidence for pinned Windows builds. Does not authorize distribution."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
FONT_BLOBS = {
    "Hack-Regular.ttf": "92a90cb06e0535afa79f6cba26ceece15a7f7959",
    "NotoEmoji-Regular.ttf": "19b7badf4afe64dae28cd64fc506b95291bb8b03",
    "Ubuntu-Light.ttf": "0e9f90d7cb13b3c816150ceae52bce1c91375e11",
    "emoji-icon-font.ttf": "806408328a74c59c8e728eeb0d4fa9ed4c84bd2f",
}
NAME_IDS = {0, 1, 5, 7, 8, 9, 11, 12, 13, 14}
LEGAL = re.compile(r"copyright|public domain|licen[cs]e|permission|redistribut|warrant", re.I)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def git_blob(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()


def bounded(data, offset, count):
    if offset < 0 or count < 0 or offset + count > len(data):
        raise ValueError("out-of-range table field")
    return data[offset:offset + count]


def font_names(data):
    # OpenType sfnt name table (not a font renderer). Preserve all relevant
    # platform/language records, without guessing an undecodable encoding.
    if bounded(data, 0, 4) not in (b"\x00\x01\x00\x00", b"OTTO", b"true"):
        raise ValueError("unsupported font container")
    count = struct.unpack(">H", bounded(data, 4, 2))[0]
    tables = []
    for i in range(count):
        tag, _, offset, length = struct.unpack(">4sIII", bounded(data, 12 + 16*i, 16))
        if tag == b"name":
            tables.append(bounded(data, offset, length))
    if len(tables) != 1:
        raise ValueError("expected exactly one font name table")
    table = tables[0]
    version, count, storage = struct.unpack(">HHH", bounded(table, 0, 6))
    if version not in (0, 1) or storage < 6 + 12*count or storage > len(table):
        raise ValueError("invalid name table directory")
    result = []
    for i in range(count):
        platform, encoding, language, name_id, length, offset = struct.unpack(
            ">HHHHHH", bounded(table, 6 + 12*i, 12))
        if name_id not in NAME_IDS:
            continue
        raw = bounded(table, storage + offset, length)
        codec = "utf-16-be" if platform in (0, 3) else "mac_roman" if platform == 1 and encoding == 0 else None
        try:
            value = raw.decode(codec) if codec else None
        except UnicodeDecodeError:
            value = None
        result.append({"platform": platform, "encoding": encoding, "language": language,
                       "name_id": name_id, "text": value,
                       **({"raw_hex": raw.hex()} if value is None else {})})
    return result


def copyright_status(records):
    if any(x["name_id"] == 0 and (x["text"] or "").strip() for x in records):
        return "decoded"
    if any(x["name_id"] == 0 for x in records):
        return "present_but_empty_or_undecodable"
    return "absent"


def leading_comments(data):
    # Capture intact leading copyright/license blocks. Do not treat the absence
    # of such a block as evidence that the file has no licensing obligations.
    text = data[:65536].decode("utf-8-sig", errors="replace")
    text = re.sub(r"^\s*<\?xml[^?]*\?>", "", text, count=1)
    pattern = re.compile(r"\s*(/\*.*?\*/|<!--.*?-->|(?://[^\n]*(?:\n|$))+)", re.S)
    out = []
    offset = 0
    while True:
        m = pattern.match(text, offset)
        if not m:
            break
        comment = m.group(1)
        if LEGAL.search(comment):
            out.append(comment)
        offset = m.end()
    return out


def sqlite_dedication_comments(data):
    # Amalgamation banners can precede the original header and preprocessor
    # directives can separate them. Preserve intact bounded comment blocks.
    text = data[:65536].decode("utf-8-sig", errors="replace")
    return [m.group(0) for m in re.finditer(r"/\*.*?\*/", text, re.S)
            if re.search(r"public\s+domain|the author disclaims copyright to this source code",
                         re.sub(r"\r?\n[ \t]*\*+[ \t]*", " ", m.group(0)), re.I)]


def file_evidence(path, base):
    raw = path.read_bytes()
    return {"path": path.relative_to(base).as_posix(), "sha256": sha256(raw),
            "size": len(raw), "leading_legal_comments": leading_comments(raw)}


def metadata():
    return json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1",
         "--filter-platform", "x86_64-pc-windows-msvc", "--features", "gui"],
        cwd=ROOT, encoding="utf-8"))


def capture_release_build(messages):
    # Capture Cargo's authoritative build-script-executed events, not guessed
    # target/release/build directory names. Keep compiler progress on stderr.
    if messages.exists():
        raise ValueError("build message destination exists")
    with messages.open("xb") as stream:
        result = subprocess.run(
            ["cargo", "build", "--locked", "--release", "--features", "gui",
             "--message-format=json-render-diagnostics"],
            cwd=ROOT, stdout=stream)
    if result.returncode:
        raise SystemExit(result.returncode)


def read_build_outputs(messages, expected_ids):
    outputs = {}
    finished = []
    with messages.open(encoding="utf-8-sig") as stream:
        for line in stream:
            if not line.strip():
                continue
            event = json.loads(line)
            reason = event.get("reason")
            if reason == "build-finished":
                finished.append(event.get("success"))
            if reason != "build-script-executed":
                continue
            package_id = event.get("package_id")
            if package_id not in expected_ids:
                continue
            directory = Path(event["out_dir"])
            if not directory.is_absolute() or not directory.is_dir():
                raise ValueError("invalid Cargo build output for " + expected_ids[package_id])
            directory = directory.resolve()
            if package_id in outputs and outputs[package_id] != directory:
                raise ValueError("ambiguous Cargo build output for " + expected_ids[package_id])
            outputs[package_id] = directory
    if finished != [True]:
        raise ValueError("Cargo messages do not describe one successful release build")
    if set(outputs) != set(expected_ids):
        raise ValueError("missing Cargo build-script event: " +
                         ", ".join(sorted(expected_ids[k] for k in expected_ids.keys() - outputs.keys())))
    generated = []
    for package_id, name in sorted(expected_ids.items(), key=lambda item: item[1]):
        directory = outputs[package_id]
        paths = sorted(directory.rglob("*.rs"))
        if not paths:
            raise ValueError("no generated Rust bindings for " + name)
        for path in paths:
            if not path.is_file() or not path.resolve().is_relative_to(directory):
                raise ValueError("invalid generated binding path")
            row = file_evidence(path, directory)
            row["package"] = name
            generated.append(row)
    return generated


def inspect(out, messages):
    if out.exists():
        raise ValueError("evidence destination exists")
    expected = json.loads((ROOT / "docs/compliance/windows-license-inventory.json").read_text(encoding="utf-8"))
    lock = sha256((ROOT / "Cargo.lock").read_bytes())
    if lock != expected["lock_sha256"]:
        raise ValueError("lockfile changed")
    meta = metadata()
    nodes = {node["id"]: node for node in meta["resolve"]["nodes"]}
    packages = {(p["name"], p["version"]): p for p in meta["packages"] if p["id"] in nodes}

    def crate(name, version):
        p = packages[(name, version)]
        if p["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError("unexpected crate source")
        return Path(p["manifest_path"]).parent, nodes[p["id"]]["features"]

    font_root, _ = crate("epaint_default_fonts", "0.31.1")
    actual = {p.name for p in (font_root / "fonts").glob("*.ttf")}
    if actual != set(FONT_BLOBS):
        raise ValueError("embedded font set differs from reviewed upstream")
    fonts = []
    for name, expected_blob in sorted(FONT_BLOBS.items()):
        raw = (font_root / "fonts" / name).read_bytes()
        if git_blob(raw) != expected_blob:
            raise ValueError("font bytes differ from exact upstream: " + name)
        names = font_names(raw)
        fonts.append({"file": name, "sha256": sha256(raw), "upstream_git_blob_sha1": expected_blob,
                      "names": names, "copyright_record_status": copyright_status(names)})

    sqlite, features = crate("libsqlite3-sys", "0.30.1")
    if "bundled" not in features or any("sqlcipher" in f for f in features):
        raise ValueError("unexpected SQLite native feature set")
    sqlite_files = [file_evidence(sqlite / "sqlite3" / name, sqlite) for name in ("sqlite3.c", "sqlite3.h")]
    for f in sqlite_files:
        raw = (sqlite / f["path"]).read_bytes()
        f["sqlite_dedication_comments"] = sqlite_dedication_comments(raw)
        if not f["sqlite_dedication_comments"]:
            raise ValueError("SQLite dedication evidence not found: " + f["path"])

    khronos, _ = crate("khronos_api", "3.1.0")
    registry = [file_evidence(path, khronos) for path in sorted(khronos.rglob("*"))
                if path.is_file() and path.suffix.lower() in (".xml", ".idl")]
    if not registry:
        raise ValueError("Khronos registry inputs not found")
    generator, _ = crate("gl_generator", "0.14.0")
    generator_headers = [file_evidence(path, generator) for path in sorted((generator / "src").rglob("*.rs"))]
    generator_headers = [f for f in generator_headers if f["leading_legal_comments"]]
    expected_ids = {packages[(name, version)]["id"]: name + "@" + version
                    for name, version in (("glutin_egl_sys", "0.7.1"),
                                          ("glutin_wgl_sys", "0.6.1"))}
    generated = read_build_outputs(messages, expected_ids)
    report = {
        "lock_sha256": lock, "scope": "review evidence, not distribution approval",
        "fonts": fonts, "sqlite": {"features": sorted(features), "files": sqlite_files},
        "khronos_registry_inputs": registry, "generator_source_headers": generator_headers,
        "generated_binding_outputs": generated,
    }
    out.mkdir(parents=True)
    (out / "embedded-evidence.json").write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    # Emit only bounded source-attribution evidence; no absolute paths,
    # environment, executable contents or user data.
    print("EMBEDDED_EVIDENCE_BEGIN")
    print(json.dumps(report, ensure_ascii=True, separators=(",", ":")))
    print("EMBEDDED_EVIDENCE_END")
    print(f"EMBEDDED_EVIDENCE_PASS fonts={len(fonts)} sqlite_headers={len(sqlite_files)} "
          f"registry_inputs={len(registry)} generated_outputs={len(generated)}")


class EvidenceTests(unittest.TestCase):
    def test_font_names(self):
        text = "Copyright fixture".encode("utf-16-be")
        table = struct.pack(">HHH", 0, 1, 18) + struct.pack(">HHHHHH", 3, 1, 1033, 0, len(text), 0) + text
        sfnt = b"\x00\x01\x00\x00" + struct.pack(">HHHH", 1, 0, 0, 0)
        sfnt += struct.pack(">4sIII", b"name", 0, 28, len(table)) + table
        self.assertEqual(font_names(sfnt)[0]["text"], "Copyright fixture")
        with self.assertRaises(ValueError):
            font_names(sfnt[:-1])
        with self.assertRaises(ValueError):
            font_names(b"bad")

    def test_copyright_status(self):
        self.assertEqual(copyright_status([]), "absent")
        self.assertEqual(copyright_status([{"name_id": 0, "text": None}]),
                         "present_but_empty_or_undecodable")
        self.assertEqual(copyright_status([{"name_id": 0, "text": "  "}]),
                         "present_but_empty_or_undecodable")
        self.assertEqual(copyright_status([{"name_id": 0, "text": "Copyright fixture"}]), "decoded")
        text = "Fixture family".encode("utf-16-be")
        table = struct.pack(">HHH", 0, 1, 18) + struct.pack(">HHHHHH", 3, 1, 1033, 1, len(text), 0) + text
        sfnt = b"\x00\x01\x00\x00" + struct.pack(">HHHH", 1, 0, 0, 0)
        sfnt += struct.pack(">4sIII", b"name", 0, 28, len(table)) + table
        self.assertEqual(copyright_status(font_names(sfnt)), "absent")

    def test_sqlite_dedication(self):
        blessing = b"/* The author disclaims copyright to this source code. */"
        self.assertEqual(sqlite_dedication_comments(b"/* banner */\n#define X 1\n" + blessing),
                         [blessing.decode()])
        self.assertEqual(sqlite_dedication_comments(b'const char *x = "public domain";'), [])
        self.assertEqual(sqlite_dedication_comments(b"/* unrelated */"), [])
        self.assertEqual(sqlite_dedication_comments(b"/* Public\n** domain */"), ["/* Public\n** domain */"])
        wrapped = b"/* The author disclaims\r\n *** copyright to this source code. */"
        self.assertEqual(sqlite_dedication_comments(wrapped), [wrapped.decode()])

    def test_build_outputs(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp).resolve()
            # Deliberately use paths unrelated to target/release/build and
            # include a space to exercise Cargo's actual reported out_dir.
            egl = base / "alternate build" / "egl"
            wgl = base / "different" / "wgl"
            egl.mkdir(parents=True)
            wgl.mkdir(parents=True)
            # Use explicit bytes: Windows text-mode writes translate LF into CRLF.
            # Exercise both formats and require preservation of the original comment.
            (egl / "egl.rs").write_bytes(b"// Copyright fixture\n")
            (wgl / "wgl.rs").write_bytes(b"// Copyright Windows fixture\r\n")
            messages = base / "messages.jsonl"
            events = [
                {"reason": "build-script-executed", "package_id": "egl-id", "out_dir": str(egl)},
                {"reason": "build-script-executed", "package_id": "wgl-id", "out_dir": str(wgl)},
                {"reason": "build-finished", "success": True},
            ]
            def write(items):
                messages.write_text("\n".join(json.dumps(item) for item in items) + "\n", encoding="utf-8")
            ids = {"egl-id": "egl@1", "wgl-id": "wgl@1"}
            write(events)
            rows = read_build_outputs(messages, ids)
            self.assertEqual([(r["package"], r["path"]) for r in rows],
                             [("egl@1", "egl.rs"), ("wgl@1", "wgl.rs")])
            self.assertEqual(rows[0]["leading_legal_comments"], ["// Copyright fixture\n"])
            self.assertEqual(rows[1]["leading_legal_comments"], ["// Copyright Windows fixture\r\n"])
            self.assertEqual(rows[0]["sha256"], sha256(b"// Copyright fixture\n"))
            self.assertEqual(rows[1]["sha256"], sha256(b"// Copyright Windows fixture\r\n"))
            write([events[0], events[2]])
            with self.assertRaisesRegex(ValueError, "missing Cargo"):
                read_build_outputs(messages, ids)
            write(events[:-1] + [{"reason": "build-finished", "success": False}])
            with self.assertRaisesRegex(ValueError, "successful release"):
                read_build_outputs(messages, ids)
            write(events)
            (wgl / "wgl.rs").unlink()
            with self.assertRaisesRegex(ValueError, "no generated Rust"):
                read_build_outputs(messages, ids)

    def test_comments(self):
        self.assertEqual(leading_comments(b'<?xml version="1.0"?>\n<!-- Copyright X -->\n<registry/>'),
                         ["<!-- Copyright X -->"])
        self.assertEqual(leading_comments(b"/* Public Domain */\nint x;"), ["/* Public Domain */"])
        self.assertEqual(leading_comments(b"// generated\nfn x() {}"), [])
        self.assertEqual(git_blob(b"hello\n"), "ce013625030ba8dba906f756967f9e9ca394464a")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-directory", type=Path)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--build-messages", type=Path)
    parser.add_argument("--capture-release-build", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(EvidenceTests)
        if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
            raise SystemExit(1)
    if args.capture_release_build:
        if not args.build_messages:
            parser.error("--capture-release-build requires --build-messages")
        capture_release_build(args.build_messages)
    if args.output_directory:
        if not args.build_messages:
            parser.error("--output-directory requires --build-messages")
        inspect(args.output_directory, args.build_messages)
    elif not (args.self_test or args.capture_release_build):
        parser.error("specify --output-directory, --capture-release-build or --self-test")
