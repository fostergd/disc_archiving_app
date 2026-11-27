"""Build/verify a review-only third-party notice ZIP, never authorize publication."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
LOCK_HASH = "60862ef1ad2cfab8812323835ad07f16294ccd4b09b6a2b32b7ea5dc5cccc567"
MANIFEST = "MANIFEST.json"
EXTRA = {
    "attribution/embedded-evidence.json",
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


def verify_bytes(items, expected):
    if set(items) != set(expected) | EXTRA:
        raise ValueError("notice bundle entry set mismatch")
    for name, (algorithm, value) in expected.items():
        raw = items[name]
        if (digest(raw) if algorithm == "sha256" else git_blob(raw)) != value:
            raise ValueError("upstream notice identity mismatch: " + name)
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
    items["attribution/EMBEDDED_ATTRIBUTION_REVIEW.md"] = (ROOT / "docs/compliance/EMBEDDED_ATTRIBUTION_REVIEW.md").read_bytes()
    for name in ("LICENSE", "provenance.json"):
        path = "upstream/angle-7403dd2/" + name
        items[path] = (ROOT / "docs/compliance" / path).read_bytes()
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
