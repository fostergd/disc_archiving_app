"""Build/verify a review-only third-party notice ZIP, never authorize publication."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import tempfile
import subprocess
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
LOCK_HASH = "60862ef1ad2cfab8812323835ad07f16294ccd4b09b6a2b32b7ea5dc5cccc567"
MANIFEST = "MANIFEST.json"
EXTRA = {
    "attribution/embedded-evidence.json",
    "attribution/windows-redistribution-index.json",
    "attribution/EMBEDDED_ATTRIBUTION_REVIEW.md",
    "upstream/angle-7403dd2/LICENSE",
    "upstream/angle-7403dd2/provenance.json",
    "README.txt",
}


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def git_blob(raw):
    return hashlib.sha1(b"blob " + str(len(raw)).encode("ascii") + b"\0" + raw).hexdigest()


def safe_name(name):
    p = PurePosixPath(name)
    if (not name or p.is_absolute() or "\\" in name or ":" in name
            or any(part in ("", ".", "..") for part in name.split("/"))):
        raise ValueError("unsafe bundle path")
    return p


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def toolchain_notices():
    # Rust standard library/compiler builtins are outside the application
    # Cargo.lock. Include official binary-distribution copyright documents
    # and REUSE license texts from the exact installed toolchain.
    version = subprocess.check_output(["rustc", "-vV"], cwd=ROOT, encoding="utf-8")
    if "commit-hash: 0ef1bc386059596dada8fb65b50425aab2ea637a" not in version.splitlines():
        raise ValueError("reviewed Rust toolchain commit changed")
    if "host: x86_64-pc-windows-msvc" not in version.splitlines():
        raise ValueError("unexpected Rust toolchain host")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], cwd=ROOT, encoding="utf-8").strip())
    doc = sysroot / "share/doc/rust"
    out = {}
    for name in ("COPYRIGHT.html", "COPYRIGHT-library.html"):
        source = doc / name
        if not source.is_file():
            raise ValueError("missing official toolchain notice: " + name)
        raw = source.read_bytes()
        if not raw or len(raw) > 32*1024*1024:
            raise ValueError("unexpected toolchain copyright document size")
        out["toolchain/distribution/" + name] = raw
    license_dir = doc / "licenses"
    sources = sorted(p for p in license_dir.rglob("*") if p.is_file())
    if not sources:
        raise ValueError("missing toolchain distribution license texts")
    for source in sources:
        if not source.resolve().is_relative_to(license_dir.resolve()):
            raise ValueError("toolchain license outside document directory")
        name = "toolchain/distribution/licenses/" + source.relative_to(license_dir).as_posix()
        safe_name(name)
        out[name] = source.read_bytes()
    provenance = load(ROOT / "docs/compliance/upstream/rust-1.100.0/provenance.json")
    for row in provenance["notices"]:
        source = ROOT / row["path"]
        raw = source.read_bytes()
        if git_blob(raw) != row["git_blob_sha1"]:
            raise ValueError("toolchain upstream license mismatch")
        name = "toolchain/source/" + row["upstream_path"]
        safe_name(name)
        out[name] = raw
    out["toolchain/source/provenance.json"] = (ROOT / "docs/compliance/upstream/rust-1.100.0/provenance.json").read_bytes()
    return out


def verify_toolchain_baseline(items, expected):
    # Compare independently recorded CI identities, not just two reads of
    # the same installed toolchain. Reject missing, extra or changed bytes.
    observed = [{"path": name, "sha256": digest(raw), "bytes": len(raw)}
                for name, raw in sorted(items.items())]
    if observed != sorted(expected, key=lambda row: row["path"]):
        raise ValueError("toolchain notice baseline differs from reviewed CI")


def expected_notices():
    inventory = load(ROOT / "docs/compliance/windows-license-inventory.json")
    if inventory["lock_sha256"] != LOCK_HASH:
        raise ValueError("reviewed lock changed")
    expected = {}
    for row in inventory["packages"]:
        for f in row["legal_files"]:
            path = f"registry/{row['name']}@{row['version']}/" + f["path"].replace("\\", "/")
            safe_name(path)
            if path in expected:
                raise ValueError("duplicate inventory notice")
            expected[path] = ("sha256", f["sha256"])
    for name in ("egui-0.31.1/provenance.json", "missing-registry-provenance.json"):
        provenance = load(ROOT / "docs/compliance/upstream" / name)
        for f in provenance["notices"]:
            path = f["path"].removeprefix("docs/compliance/")
            safe_name(path)
            if path in expected:
                raise ValueError("duplicate upstream notice")
            expected[path] = ("git_blob", f["git_blob_sha1"])
    if len(expected) != 322:
        raise ValueError("notice inventory count changed")
    return expected


def verify_index(index, inventory, runtime, mappings, fonts, items):
    # Index is a reviewed navigation/terms record, not publication approval.
    if (index.get("schema") != 1 or index.get("lock_sha256") != inventory["lock_sha256"]
            or index.get("distribution_approved") is not False):
        raise ValueError("redistribution index identity mismatch")
    rows = index["packages"]
    identities = {p["name"]+"@"+p["version"]:p for p in inventory["packages"]}
    if len(rows) != len(identities) or {p["package"] for p in rows} != set(identities):
        raise ValueError("redistribution index coverage mismatch")
    compiled = {p["name"]+"@"+p["version"] for p in runtime["compiled_packages"]}
    only = {p["name"]+"@"+p["version"] for p in runtime["candidate_only_packages"]}
    # Runtime includes our first-party binary/library, unlike registry inventory.
    # Exclude only the exact reviewed application identity, never arbitrary extras.
    first_party = "disc-archive@0.1.0"
    if first_party not in compiled or first_party in only or first_party in identities:
        raise ValueError("first-party runtime identity mismatch")
    compiled.remove(first_party)
    if compiled & only or compiled | only != set(identities):
        raise ValueError("redistribution scope mismatch")
    supplemental = {m["package"]:{"upstream/"+n for n in m["notices"]} for m in mappings}
    for row in rows:
        key = row["package"]
        original = identities[key]
        expected_paths = {"registry/"+key+"/"+f["path"].replace(chr(92),"/") for f in original["legal_files"]}
        expected_paths |= supplemental.get(key,set())
        if key.startswith("epaint_default_fonts@"):
            expected_paths |= {n["path"].removeprefix("docs/compliance/") for n in fonts}
        if (row["declared_license"] != original["license"] or not row["proposed_terms"]
                or row["build_scope"] != ("compiled_candidate" if key in compiled else "inventory_only")
                or not expected_paths or set(row["notice_paths"]) != expected_paths
                or len(row["notice_paths"]) != len(expected_paths)
                or not expected_paths <= set(items)):
            raise ValueError("redistribution index package evidence mismatch: "+key)
    if not set(index["additional_attribution"]) <= set(items):
        raise ValueError("redistribution index additional attribution missing")


def verify_bytes(items, expected):
    toolchain = toolchain_notices()
    baseline = load(ROOT / "docs/compliance/toolchain-notice-baseline.json")
    if baseline["toolchain_commit"] != "0ef1bc386059596dada8fb65b50425aab2ea637a":
        raise ValueError("unexpected toolchain baseline commit")
    verify_toolchain_baseline(toolchain, baseline["files"])
    if set(items) != set(expected) | EXTRA | set(toolchain):
        raise ValueError("notice bundle entry set mismatch")
    for name, (algorithm, value) in expected.items():
        raw = items[name]
        if (digest(raw) if algorithm == "sha256" else git_blob(raw)) != value:
            raise ValueError("upstream notice identity mismatch: " + name)
    for name, raw in toolchain.items():
        if items[name] != raw:
            raise ValueError("toolchain notice identity mismatch: " + name)
    angle = items["upstream/angle-7403dd2/LICENSE"]
    if git_blob(angle) != "0f65fd60fd68d465a2283378849f16532f5ee4f1":
        raise ValueError("ANGLE license identity mismatch")
    for name in ("upstream/angle-7403dd2/provenance.json", "attribution/EMBEDDED_ATTRIBUTION_REVIEW.md"):
        source = ROOT / ("docs/compliance/" + (name if name.startswith("upstream/") else name.split("/")[-1]))
        if items[name] != source.read_bytes():
            raise ValueError("review document mismatch: " + name)
    report = json.loads(items["attribution/embedded-evidence.json"])
    baseline = load(ROOT / "docs/compliance/embedded-evidence-xml.json")["evidence"]
    if report != baseline or report["lock_sha256"] != LOCK_HASH:
        raise ValueError("embedded attribution differs from reviewed baseline")
    index_path = ROOT / "docs/compliance/windows-redistribution-index.json"
    index_raw = items["attribution/windows-redistribution-index.json"]
    if index_raw != index_path.read_bytes():
        raise ValueError("redistribution index differs from reviewed source")
    verify_index(json.loads(index_raw),
                 load(ROOT / "docs/compliance/windows-license-inventory.json"),
                 load(ROOT / "docs/compliance/runtime-evidence-baseline.json")["evidence"],
                 load(ROOT / "docs/compliance/upstream/missing-registry-provenance.json")["mappings"],
                 load(ROOT / "docs/compliance/upstream/egui-0.31.1/provenance.json")["notices"],
                 items)
    if not items["README.txt"].startswith(b"REVIEW ONLY"):
        raise ValueError("review-only warning missing")


def write_bundle(destination, items):
    entries = {name: digest(raw) for name, raw in sorted(items.items())}
    manifest = {"schema": 1, "lock_sha256": LOCK_HASH, "distribution_approved": False, "files": entries}
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED) as z:
        for name, raw in sorted(items.items()):
            safe_name(name)
            z.writestr(name, raw)
        z.writestr(MANIFEST, json.dumps(manifest, indent=2) + "\n")


def read_bundle(path):
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        if len(names) != len(set(n.casefold() for n in names)) or MANIFEST not in names:
            raise ValueError("duplicate or missing manifest")
        for name in names:
            safe_name(name)
        manifest = json.loads(z.read(MANIFEST))
        items = {name: z.read(name) for name in names if name != MANIFEST}
    if manifest != {"schema": 1, "lock_sha256": LOCK_HASH, "distribution_approved": False,
                    "files": {name: digest(raw) for name, raw in items.items()}}:
        raise ValueError("bundle manifest mismatch")
    return items


def build(notice_dir, embedded, destination):
    expected = expected_notices()
    report = load(notice_dir / "review-evidence.json")
    if (report["lock_sha256"] != LOCK_HASH or report["copied_notice_count"] != 322
            or report["upstream_mapped_missing_count"] != 11
            or report["public_distribution_approved"] is not False):
        raise ValueError("notice collection report mismatch")
    items = {}
    for row in report["files"]:
        name = row["path"].replace("\\", "/")
        path = safe_name(name)
        if name in items:
            raise ValueError("duplicate collected notice")
        source = notice_dir.joinpath(*path.parts)
        if not source.resolve().is_relative_to(notice_dir.resolve()):
            raise ValueError("notice outside collection")
        raw = source.read_bytes()
        if digest(raw) != row["sha256"]:
            raise ValueError("collected notice changed")
        items[name] = raw
    items["attribution/embedded-evidence.json"] = embedded.read_bytes()
    items["attribution/windows-redistribution-index.json"] = (ROOT / "docs/compliance/windows-redistribution-index.json").read_bytes()
    items["attribution/EMBEDDED_ATTRIBUTION_REVIEW.md"] = (ROOT / "docs/compliance/EMBEDDED_ATTRIBUTION_REVIEW.md").read_bytes()
    for name in ("LICENSE", "provenance.json"):
        path = "upstream/angle-7403dd2/" + name
        items[path] = (ROOT / "docs/compliance" / path).read_bytes()
    toolchain = toolchain_notices()
    items.update(toolchain)
    print("TOOLCHAIN_NOTICE_IDENTITIES " + json.dumps(
        [{"path": name, "sha256": digest(raw), "bytes": len(raw)}
         for name, raw in sorted(toolchain.items())], separators=(",", ":")))
    items["README.txt"] = (b"REVIEW ONLY - public distribution is not authorized by this bundle.\n"
                           b"Original third-party notices and attribution evidence. See project THIRD_PARTY_NOTICES.md.\n"
                           b"Inventory conservatively includes build/dev candidates; not a binary SBOM.\n")
    verify_bytes(items, expected)
    write_bundle(destination, items)
    verify(destination)


def verify(path):
    items = read_bundle(path)
    verify_bytes(items, expected_notices())
    print(f"NOTICE_BUNDLE_REVIEW_PASS files={len(items)} distribution_approved=false")


class BundleTests(unittest.TestCase):
    def test_roundtrip_and_tamper(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "bundle.zip"
            write_bundle(p, {"notice.txt": b"original\r\n"})
            self.assertEqual(read_bundle(p), {"notice.txt": b"original\r\n"})
            with self.assertRaises(FileExistsError):
                write_bundle(p, {})
            bad = Path(tmp) / "bad.zip"
            with zipfile.ZipFile(p) as source, zipfile.ZipFile(bad, "x") as out:
                out.writestr(MANIFEST, source.read(MANIFEST))
                out.writestr("notice.txt", b"modified")
            with self.assertRaisesRegex(ValueError, "manifest mismatch"):
                read_bundle(bad)

    def test_toolchain_baseline(self):
        items = {"toolchain/a.txt": b"notice", "toolchain/b.txt": b"other"}
        expected = [{"path": name, "sha256": digest(raw), "bytes": len(raw)}
                    for name, raw in items.items()]
        verify_toolchain_baseline(items, expected)
        for changed in ({"toolchain/a.txt": b"notice"},
                        {**items, "toolchain/c.txt": b"extra"},
                        {**items, "toolchain/a.txt": b"tamper"}):
            with self.assertRaisesRegex(ValueError, "baseline differs"):
                verify_toolchain_baseline(changed, expected)

    def test_redistribution_index(self):
        inventory = {"lock_sha256":"fixture","packages":[{"name":"a","version":"1","license":"MIT","legal_files":[{"path":"LICENSE"}]}]}
        runtime = {"compiled_packages":[{"name":"a","version":"1"},{"name":"disc-archive","version":"0.1.0"}],"candidate_only_packages":[]}
        path = "registry/a@1/LICENSE"
        row = {"package":"a@1","declared_license":"MIT","proposed_terms":"MIT","build_scope":"compiled_candidate","notice_paths":[path]}
        index = {"schema":1,"lock_sha256":"fixture","distribution_approved":False,"packages":[row],"additional_attribution":[]}
        verify_index(index, inventory, runtime, [], [], {path:b"fixture"})
        for bad_runtime in (
            {**runtime, "compiled_packages": [{"name":"a","version":"1"}]},
            {**runtime, "compiled_packages": runtime["compiled_packages"] + [{"name":"unknown","version":"1"}]},
            {**runtime, "candidate_only_packages": [{"name":"disc-archive","version":"0.1.0"}]},
        ):
            with self.assertRaises(ValueError):
                verify_index(index, inventory, bad_runtime, [], [], {path:b"fixture"})
        for altered in ({**index,"packages":[]},{**index,"packages":[{**row,"notice_paths":["wrong"]}]},
                        {**index,"distribution_approved":True}):
            with self.assertRaises(ValueError):
                verify_index(altered, inventory, runtime, [], [], {path:b"fixture"})
        with self.assertRaises(ValueError):
            verify_index(index, inventory, runtime, [], [], {})

    def test_path_validation(self):
        for name in ("../bad", "/bad", "a//b", "a\\b", "C:/bad", "a/./b"):
            with self.assertRaises(ValueError):
                safe_name(name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--notice-directory", type=Path)
    parser.add_argument("--embedded-report", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify", type=Path)
    args = parser.parse_args()
    if args.self_test:
        if not unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(BundleTests)).wasSuccessful():
            raise SystemExit(1)
    elif args.verify:
        verify(args.verify)
    elif args.notice_directory and args.embedded_report and args.output:
        build(args.notice_directory, args.embedded_report, args.output)
    else:
        parser.error("specify build inputs, --verify, or --self-test")
