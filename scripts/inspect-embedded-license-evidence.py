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
    if not any(x["name_id"] == 0 and x["text"] for x in result):
        raise ValueError("font has no decoded copyright record")
    return result


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


def file_evidence(path, base):
    raw = path.read_bytes()
    return {"path": path.relative_to(base).as_posix(), "sha256": sha256(raw),
            "size": len(raw), "leading_legal_comments": leading_comments(raw)}


def metadata():
    return json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1",
         "--filter-platform", "x86_64-pc-windows-msvc", "--features", "gui"],
        cwd=ROOT, encoding="utf-8"))


def inspect(out):
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
        fonts.append({"file": name, "sha256": sha256(raw), "upstream_git_blob_sha1": expected_blob,
                      "names": font_names(raw)})

    sqlite, features = crate("libsqlite3-sys", "0.30.1")
    if "bundled" not in features or any("sqlcipher" in f for f in features):
        raise ValueError("unexpected SQLite native feature set")
    sqlite_files = [file_evidence(sqlite / "sqlite3" / name, sqlite) for name in ("sqlite3.c", "sqlite3.h")]
    if not all(any("public domain" in c.lower() for c in f["leading_legal_comments"]) for f in sqlite_files):
        raise ValueError("SQLite public-domain header not found")

    khronos, _ = crate("khronos_api", "3.1.0")
    registry = [file_evidence(path, khronos) for path in sorted(khronos.rglob("*"))
                if path.is_file() and path.suffix.lower() in (".xml", ".idl")]
    if not registry:
        raise ValueError("Khronos registry inputs not found")
    generator, _ = crate("gl_generator", "0.14.0")
    generator_headers = [file_evidence(path, generator) for path in sorted((generator / "src").rglob("*.rs"))]
    generator_headers = [f for f in generator_headers if f["leading_legal_comments"]]
    target = Path(meta["target_directory"]) / "release" / "build"
    generated = []
    for name in ("glutin_egl_sys", "glutin_wgl_sys"):
        paths = sorted(target.glob(name + "-*/out/*.rs"))
        if not paths:
            raise ValueError("generated binding output not found: " + name)
        generated.extend(file_evidence(path, target) for path in paths)
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
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(EvidenceTests)
        if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
            raise SystemExit(1)
    if args.output_directory:
        inspect(args.output_directory)
    elif not args.self_test:
        parser.error("specify --output-directory or --self-test")
