"""Read-only, bounded inspection of the old public preview. Never execute assets."""
import argparse
import hashlib
import io
import json
from pathlib import Path
from collections import Counter
import re
import subprocess
import struct
import unittest
import urllib.error
import urllib.request
import zipfile

BASE = ("https://github.com/fostergd/disc_archiving_app/releases/download/"
        "preview-96e0ec3dc921-45907196171-1/")
ZIP = "disc-archive-windows-x64-preview.zip"
SIDECAR = ZIP + ".sha256"
IDENTITIES = {
    ZIP: (6220532, "c3ce19d5f985db8d620599b5f2da08faac640ae1b6eb1ab96cc6b4b9f1a26dca"),
    SIDECAR: (104, "da28a8aabfbcee4a43d90c5124078cb80ffb05b7456106c18700b98b5e1fd052"),
}
PAYLOAD = {"disc-archive.exe", "disc-archive-studio.exe", "README.md", "WINDOWS_ACCEPTANCE.md"}
PATTERNS = {
    "private_key": re.compile(rb"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----"),
    "github_token": re.compile(rb"(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})"),
    "aws_key": re.compile(rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    "private_ip": re.compile(rb"(?<![0-9.])(?:192\.168|10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}(?![0-9.])"),
    "local_user_path": re.compile(rb"(?i)(?:[a-z]:\\Users\\[^\\\r\n\"' ]+|/home/[^/\r\n\"' ]+/)"),
    "email": re.compile(rb"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
}


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def download(name):
    size, sha = IDENTITIES[name]
    # Public assets only: no token, no credentials, no arbitrary URL input.
    request = urllib.request.Request(BASE + name, headers={"User-Agent": "Disc-Archive-Studio-review"})
    with urllib.request.urlopen(request, timeout=45) as response:
        if not response.geturl().startswith("https://"):
            raise ValueError("non-HTTPS asset response")
        raw = response.read(size + 1)
    if len(raw) != size or digest(raw) != sha:
        raise ValueError("legacy asset identity mismatch")
    return raw


def variants_of(raw):
    # Scan bytes plus ASCII-compatible UTF-16LE/BE strings. Counts only.
    variants = [raw]
    for endian in ("utf-16-le", "utf-16-be"):
        for offset in (0, 1):
            data = raw[offset:]
            variants.append(data[:len(data)//2*2].decode(endian, errors="replace").encode("ascii", errors="replace"))
    return variants


def scan(raw):
    variants = variants_of(raw)
    return {key: sum(len(pattern.findall(v)) for v in variants) for key, pattern in PATTERNS.items()}


def pe_section_context(raw):
    # Report only coarse section types, encoding and occurrence counts.
    # Never output matched bytes, addresses, offsets or surrounding strings.
    if len(raw) < 64 or raw[:2] != b"MZ":
        raise ValueError("missing DOS header")
    pe = struct.unpack_from("<I", raw, 60)[0]
    if pe+24 > len(raw) or raw[pe:pe+4] != b"PE\0\0":
        raise ValueError("missing PE header")
    machine, count = struct.unpack_from("<HH", raw, pe+4)
    optional = struct.unpack_from("<H", raw, pe+20)[0]
    table = pe+24+optional
    if machine != 0x8664 or not 1 <= count <= 96 or table+count*40 > len(raw):
        raise ValueError("unexpected PE section layout")
    ranges = []
    for index in range(count):
        off = table+40*index
        size, ptr = struct.unpack_from("<II", raw, off+16)
        flags = struct.unpack_from("<I", raw, off+36)[0]
        if not size:
            continue
        if ptr < table+count*40 or ptr+size > len(raw):
            raise ValueError("invalid section bounds")
        if flags & 0x20000000:
            kind = "executable_section"
        elif flags & 0x80000000:
            kind = "writable_section"
        else:
            kind = "non_executable_readonly_section"
        ranges.append((ptr,ptr+size,kind))
    ranges.sort()
    if any(a[1] > b[0] for a,b in zip(ranges,ranges[1:])):
        raise ValueError("overlapping PE sections")
    return ranges


def email_binary_context(raw):
    ranges = pe_section_context(raw)
    counts = Counter()
    # Build an offset-preserving view for each encoding/alignment.
    # ASCII-range bytes map to themselves; all others are separators.
    # No Unicode surrogate contraction or offset ambiguity is possible.
    views = [("byte", raw, 1, 0)]
    for endian in ("le","be"):
        for align in (0,1):
            chunk = raw[align:]
            chunk = chunk[:len(chunk)//2*2]
            low, high = (chunk[::2],chunk[1::2]) if endian == "le" else (chunk[1::2],chunk[::2])
            view = bytes(a if b == 0 and a < 128 else 0 for a,b in zip(low,high))
            views.append(("utf16"+endian,view,2,align))
    for enc, view, stride, align in views:
        for match in PATTERNS["email"].finditer(view):
            start,end = align+stride*match.start(),align+stride*match.end()
            kinds = [kind for lo,hi,kind in ranges if lo <= start and end <= hi]
            kind = kinds[0] if len(kinds)==1 else "outside_or_crossing_sections"
            before = view[match.start()-1] if match.start() else 0
            after = view[match.end()] if match.end()<len(view) else 0
            boundary = "nul_both" if before==0 and after==0 else "other_boundaries"
            counts[enc+"_"+kind+"_"+boundary] += 1
    original = scan(raw)["email"]
    return {"context_counts":dict(sorted(counts.items())),
            "original_pattern_occurrences":original,
            "context_occurrences":sum(counts.values()),
            "counts_agree":original==sum(counts.values()),
            "scope":"PE section permissions and encoding boundaries only; no attribution or privacy clearance"}


def classify_review(raw):
    # Identify only the exact public hosted-runner Cargo/rustup directory
    # prefixes, not arbitrary accounts, drives, or paths. Never emit matches.
    counts = Counter()
    for v in variants_of(raw):
        for m in PATTERNS["local_user_path"].finditer(v):
            prefix = v[m.start():m.start()+160].lower()
            if re.match(rb"[a-z]:\\users\\runneradmin\\\.cargo\\registry\\src\\", prefix):
                counts["hosted_runner_registry_prefix"] += 1
            elif re.match(rb"[a-z]:\\users\\runneradmin\\\.rustup\\toolchains\\", prefix):
                counts["hosted_runner_toolchain_prefix"] += 1
            else:
                counts["local_path_unclassified"] += 1
        for m in PATTERNS["email"].finditer(v):
            # An email-shaped string in executable data is not proof of
            # attribution or personal exposure. Leave it unresolved unless
            # independently established; never print it or a hash of it.
            counts["email_unclassified"] += 1
    return dict(sorted(counts.items()))


def compare_email_sources(values, packages, max_bytes=256*1024*1024):
    # Candidate source correspondence is evidence, not automatic clearance.
    # Only emit package identities, not matches, hashes of addresses or paths.
    results = {value: set() for value in values}
    total = 0
    skipped = 0
    for package, root in sorted(packages.items()):
        for path in sorted(root.rglob("*")):
            if path.suffix.lower() not in (".rs", ".c", ".h", ".cpp", ".toml", ".md", ".txt", ".xml") and not path.name.upper().startswith(("LICENSE", "NOTICE", "COPYRIGHT")):
                continue
            if not path.is_file() or not path.resolve().is_relative_to(root.resolve()):
                continue
            size = path.stat().st_size
            if size > 16*1024*1024 or total + size > max_bytes:
                skipped += 1
                continue
            raw = path.read_bytes()
            total += len(raw)
            for match in PATTERNS["email"].finditer(raw):
                value = match.group()
                if value in results:
                    results[value].add(package)
    return {
        "distinct_email_patterns": len(results),
        "distinct_patterns_with_candidate_source_match": sum(bool(v) for v in results.values()),
        "distinct_patterns_without_candidate_source_match": sum(not v for v in results.values()),
        "matching_candidate_packages": sorted(set().union(*results.values()) if results else set()),
        "scanned_bytes": total,
        "skipped_files": skipped,
        "scope": "exact email-pattern correspondence in locked compiled registry candidates; no assertion of attribution or privacy clearance",
    }


def registry_candidates(runtime_report):
    root = Path(__file__).resolve().parents[1]
    inventory = json.loads((root/"docs/compliance/windows-license-inventory.json").read_text(encoding="utf-8"))
    if hashlib.sha256((root/"Cargo.lock").read_bytes()).hexdigest() != inventory["lock_sha256"]:
        raise ValueError("source comparison lock mismatch")
    # The release build already ran against this locked graph.
    report = json.loads(runtime_report.read_text(encoding="utf-8"))
    if report["lock_sha256"] != inventory["lock_sha256"]:
        raise ValueError("runtime source comparison lock mismatch")
    compiled = {(p["name"], p["version"]) for p in report["compiled_packages"]}
    proc = subprocess.run(["cargo","metadata","--locked","--offline","--format-version","1","--filter-platform","x86_64-pc-windows-msvc","--features","gui"],cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    if proc.returncode:
        raise ValueError("locked metadata retrieval failed")
    metadata = json.loads(proc.stdout)
    packages = {}
    for p in metadata["packages"]:
        key = (p["name"],p["version"])
        if key not in compiled or not (p.get("source") or "").startswith("registry+"):
            continue
        manifest = Path(p["manifest_path"])
        if not manifest.is_file() or manifest.name != "Cargo.toml":
            raise ValueError("candidate source manifest missing")
        label = p["name"]+"@"+p["version"]
        if label in packages:
            raise ValueError("ambiguous candidate package")
        packages[label] = manifest.parent
    if not packages:
        raise ValueError("no compiled registry candidates")
    return packages


def inspect_current(directory, runtime_report=None):
    entries = []
    emails = set()
    for name in sorted(n for n in PAYLOAD if n.endswith(".exe")):
        path = directory / name
        if not path.is_file() or not path.resolve().is_relative_to(directory.resolve()):
            raise ValueError("current executable missing or outside release directory")
        if path.stat().st_size > 64 * 1024 * 1024:
            raise ValueError("current executable exceeds inspection bound")
        raw = path.read_bytes()
        if not raw.startswith(b"MZ"):
            raise ValueError("current executable header missing")
        counts = scan(raw)
        for variant in variants_of(raw):
            emails.update(m.group() for m in PATTERNS["email"].finditer(variant))
        entries.append({"name": name, "bytes": len(raw), "sha256": digest(raw),
                        "pattern_counts": counts, "review_classes": classify_review(raw),
                        "email_binary_context": email_binary_context(raw)})
    report = {"entries": entries, "scope": "new release executables; heuristic counts only",
              "publication_approved": False}
    if runtime_report is not None:
        report["email_source_comparison"] = compare_email_sources(emails, registry_candidates(runtime_report))
    print("CURRENT_BINARY_EXPOSURE_REVIEW " + json.dumps(report, sort_keys=True, separators=(",", ":")))
    if any(row["pattern_counts"][k] for row in entries for k in ("private_key","github_token","aws_key")):
        raise ValueError("possible credential in current executable; review required")



def inspect_zip(raw):
    with zipfile.ZipFile(io.BytesIO(raw)) as z:
        names = z.namelist()
        if len(names) != len(set(n.casefold() for n in names)):
            raise ValueError("duplicate ZIP entry")
        if set(names) != PAYLOAD | {"SHA256SUMS.txt"}:
            raise ValueError("unexpected legacy ZIP layout")
        if sum(i.file_size for i in z.infolist()) > 64 * 1024 * 1024:
            raise ValueError("legacy ZIP exceeds inspection bound")
        entries = {name: z.read(name) for name in names}
    lines = entries["SHA256SUMS.txt"].decode("ascii").splitlines()
    expected = {digest(entries[name]) + "  " + name for name in PAYLOAD}
    if len(lines) != len(expected) or set(lines) != expected:
        raise ValueError("legacy internal checksums mismatch")
    for name in ("disc-archive.exe", "disc-archive-studio.exe"):
        if not entries[name].startswith(b"MZ"):
            raise ValueError("legacy executable header missing")
    return {
        "entries": [{"name": name, "bytes": len(data), "sha256": digest(data),
                     "pattern_counts": scan(data), "review_classes": classify_review(data),
                     **({"email_binary_context": email_binary_context(data)}
                        if name.endswith(".exe") and data[:2] == b"MZ" and len(data) >= 64 else {})}
                    for name, data in sorted(entries.items())],
        "internal_checksums_valid": True,
        "third_party_notice_bundle_present": False,
        "publication_approved": False,
    }


def inspect():
    raw = download(ZIP)
    sidecar = download(SIDECAR)
    if sidecar.decode("ascii").strip() != digest(raw) + "  " + ZIP:
        raise ValueError("legacy ZIP sidecar mismatch")
    report = inspect_zip(raw)
    report.update({"asset_sha256": digest(raw), "sidecar_sha256": digest(sidecar),
                   "scope": "legacy public asset byte identity, checksums and heuristic screening; no execution"})
    print("LEGACY_PREVIEW_REVIEW " + json.dumps(report, sort_keys=True, separators=(",", ":")))
    print("LEGACY_PREVIEW_REVIEW_COMPLETE: legacy package lacks third-party notice bundle; not approved")


class Tests(unittest.TestCase):
    def fixture(self, tamper=False, extra=False):
        entries = {name: b"MZfixture" if name.endswith(".exe") else b"fixture" for name in PAYLOAD}
        lines = [digest(data) + "  " + name for name, data in entries.items()]
        entries["SHA256SUMS.txt"] = ("\n".join(lines) + "\n").encode("ascii")
        if tamper:
            entries["README.md"] = b"changed"
        if extra:
            entries["unexpected.txt"] = b"extra"
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as z:
            for name, data in entries.items():
                z.writestr(name, data)
        return stream.getvalue()

    def test_layout_and_checksums(self):
        report = inspect_zip(self.fixture())
        self.assertTrue(report["internal_checksums_valid"])
        self.assertFalse(report["publication_approved"])
        with self.assertRaisesRegex(ValueError, "checksums mismatch"):
            inspect_zip(self.fixture(tamper=True))
        with self.assertRaisesRegex(ValueError, "layout"):
            inspect_zip(self.fixture(extra=True))

    def test_review_classes(self):
        runner = b"C:" + b"\\Users\\" + b"runneradmin\\.cargo\\registry\\src\\crate\\lib.rs"
        other = b"C:" + b"\\Users\\" + b"fixture\\private"
        self.assertEqual(classify_review(runner)["hosted_runner_registry_prefix"], 1)
        self.assertEqual(classify_review(other)["local_path_unclassified"], 1)
        self.assertEqual(classify_review(b"fixture" + b"@example.invalid")["email_unclassified"], 1)

    def test_pe_email_context(self):
        raw = bytearray(1024)
        raw[:2] = b"MZ"
        struct.pack_into("<I",raw,60,64)
        raw[64:68] = b"PE\0\0"
        struct.pack_into("<HH",raw,68,0x8664,1)
        struct.pack_into("<H",raw,84,240)
        off=88+240
        struct.pack_into("<II",raw,off+16,512,512)
        struct.pack_into("<I",raw,off+36,0x60000020)
        sample = b"fixture" + b"@example.invalid"
        raw[520:520+len(sample)] = sample
        result = email_binary_context(bytes(raw))
        self.assertEqual(result["context_counts"],{"byte_executable_section_nul_both":1})
        self.assertTrue(result["counts_agree"])
        self.assertNotIn(sample.decode(),json.dumps(result))
        with self.assertRaises(ValueError):
            pe_section_context(bytes(raw[:600]))

    def test_source_comparison(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            address = b"fixture" + b"@example.invalid"
            (root/"lib.rs").write_bytes(address)
            other = b"unmatched" + b"@example.invalid"
            result = compare_email_sources({address,other},{"fixture@1.0":root})
            self.assertEqual(result["distinct_patterns_with_candidate_source_match"],1)
            self.assertEqual(result["distinct_patterns_without_candidate_source_match"],1)
            self.assertEqual(result["matching_candidate_packages"],["fixture@1.0"])
            self.assertNotIn(address.decode(),json.dumps(result))
            self.assertEqual(compare_email_sources({address},{"fixture@1.0":root},max_bytes=0)["skipped_files"],1)

    def test_encoded_patterns(self):
        sample = "gh" + "p_" + "Z" * 36
        self.assertGreater(scan(sample.encode("utf-16-le"))["github_token"], 0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--current-release-directory", type=Path)
    parser.add_argument("--runtime-report", type=Path)
    args = parser.parse_args()
    if args.self_test:
        if not unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests)).wasSuccessful():
            raise SystemExit(1)
    elif args.current_release_directory:
        try:
            inspect_current(args.current_release_directory, args.runtime_report)
        except (ValueError, OSError):
            print("CURRENT_BINARY_EXPOSURE_REVIEW_INCOMPLETE: review required")
            raise SystemExit(1) from None
    else:
        try:
            inspect()
        except (urllib.error.URLError, TimeoutError):
            # Asset can be withdrawn by owner; never make a build pass imply
            # that inaccessible assets were inspected or cleared.
            print("LEGACY_PREVIEW_REVIEW_INCOMPLETE: asset retrieval unavailable; publication remains blocked")
        except (ValueError, OSError, zipfile.BadZipFile):
            print("LEGACY_PREVIEW_REVIEW_INCOMPLETE: asset identity or format validation failed")
            raise SystemExit(1) from None
