"""Summarize the pinned Rust library copyright report without emitting author data."""
import argparse
from collections import Counter
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
REPORT = "toolchain/distribution/COPYRIGHT-library.html"
COMMIT = "0ef1bc386059596dada8fb65b50425aab2ea637a"


class ReportParser(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.section = None
        self.capture = None
        self.parts = []
        self.sections = set()
        self.in_tree = Counter()
        self.dependencies = []
        self.notice_blocks = 0

    def handle_starttag(self, tag, attrs):
        if tag == "h2":
            self.section = dict(attrs).get("id")
            if self.section:
                self.sections.add(self.section)
        if self.section not in ("in-tree-files", "out-of-tree-dependencies"):
            return
        if tag in ("h3", "p"):
            self.capture = tag
            self.parts = []
        if tag == "pre" and self.section == "out-of-tree-dependencies":
            self.notice_blocks += 1
            if not self.dependencies:
                raise ValueError("notice before dependency")
            self.dependencies[-1]["notice_blocks"] += 1

    def handle_data(self, data):
        if self.capture:
            self.parts.append(data)

    def handle_endtag(self, tag):
        if tag != self.capture:
            return
        text = " ".join("".join(self.parts).split())
        self.capture = None
        if tag == "h3" and self.section == "out-of-tree-dependencies":
            # Official template prefixes the crate name/version with a package emoji.
            label = text.removeprefix("\U0001f4e6").strip()
            if not re.fullmatch(r"[A-Za-z0-9_.+-]+", label):
                raise ValueError("unexpected dependency heading")
            self.dependencies.append({"crate": label, "license": None, "notice_blocks": 0})
        elif tag == "p" and text.startswith("License:"):
            expr = text[len("License:"):].strip()
            # Only permit SPDX-like declarations in the public report.
            if not expr or len(expr) > 256 or not re.fullmatch(r"[A-Za-z0-9_.+(): /-]+", expr):
                raise ValueError("unrecognized license declaration format")
            if self.section == "in-tree-files":
                self.in_tree[expr] += 1
            elif self.section == "out-of-tree-dependencies":
                if not self.dependencies or self.dependencies[-1]["license"] is not None:
                    raise ValueError("missing dependency or duplicate license")
                self.dependencies[-1]["license"] = expr

    def result(self):
        if not {"in-tree-files", "out-of-tree-dependencies"} <= self.sections:
            raise ValueError("library copyright sections missing")
        if not self.in_tree or not self.dependencies:
            raise ValueError("empty library copyright sections")
        if any(row["license"] is None for row in self.dependencies):
            raise ValueError("dependency license missing")
        if len({row["crate"] for row in self.dependencies}) != len(self.dependencies):
            raise ValueError("duplicate dependency heading")
        return {
            "in_tree_license_declarations": dict(sorted(self.in_tree.items())),
            "out_of_tree_dependencies": self.dependencies,
            "out_of_tree_license_declarations": dict(sorted(Counter(
                row["license"] for row in self.dependencies).items())),
            "out_of_tree_notice_blocks": self.notice_blocks,
        }


def summarize(raw):
    parser = ReportParser()
    parser.feed(raw.decode("utf-8"))
    parser.close()
    return parser.result()


def target_library_inventory(directory, dependencies):
    # The official report covers more platforms and build/dev tools than
    # this installed target. Record archive presence, not proof of linkage.
    names = set()
    archives = []
    for path in sorted(directory.glob("*.rlib")):
        match = re.fullmatch(r"lib([A-Za-z0-9_]+)-([a-f0-9]+)\.rlib", path.name)
        if not match or not path.is_file() or not path.resolve().is_relative_to(directory.resolve()):
            raise ValueError("unexpected installed target archive")
        name = match.group(1)
        names.add(name)
        archives.append({"crate": name, "filename": path.name, "bytes": path.stat().st_size})
    if not {"std", "core", "alloc", "compiler_builtins"} <= names:
        raise ValueError("required Windows standard library archives missing")
    exceptional = {"fortanix_sgx_abi", "r_efi", "r_efi_alloc"}
    if names & exceptional:
        raise ValueError("unexpected SGX/UEFI archive in Windows target; review required")
    def crate_name(label):
        # Split on the first hyphen followed by a numeric version.
        return re.split(r"-(?=\d)", label, maxsplit=1)[0].replace("-", "_")
    return {
        "archives": archives,
        "matching_report_candidates": sorted(row["crate"] for row in dependencies
                                             if crate_name(row["crate"]) in names),
        "report_only_candidates": sorted(row["crate"] for row in dependencies
                                         if crate_name(row["crate"]) not in names),
        "sgx_uefi_archive_names_present": [],
        "scope": "installed Windows target rlib filenames only; not exact linkage or version matching",
    }


def verify_report_baseline(report, expected):
    if report != expected:
        raise ValueError("standard library declarations differ from verified report baseline")


def inspect():
    base = json.loads((ROOT/"docs/compliance/toolchain-notice-baseline.json").read_text(encoding="utf-8"))
    if base["toolchain_commit"] != COMMIT:
        raise ValueError("unexpected toolchain baseline")
    expected = [row for row in base["files"] if row["path"] == REPORT]
    if len(expected) != 1:
        raise ValueError("library report identity missing")
    version = subprocess.check_output(["rustc","-vV"],cwd=ROOT,encoding="utf-8")
    if "commit-hash: "+COMMIT not in version.splitlines():
        raise ValueError("wrong Rust toolchain")
    sysroot = Path(subprocess.check_output(["rustc","--print","sysroot"],cwd=ROOT,encoding="utf-8").strip())
    raw = (sysroot/"share/doc/rust/COPYRIGHT-library.html").read_bytes()
    if len(raw) != expected[0]["bytes"] or hashlib.sha256(raw).hexdigest() != expected[0]["sha256"]:
        raise ValueError("library copyright report differs from baseline")
    report = {"source_sha256": expected[0]["sha256"], "scope": "official standard-library distribution report; not exact executable linkage",
              **summarize(raw), "distribution_approved": False}
    baseline = json.loads((ROOT/"docs/compliance/rust-library-report-baseline.json").read_text(encoding="utf-8"))
    verify_report_baseline(report, baseline["evidence"])
    target_dir = Path(subprocess.check_output(
        ["rustc","--print","target-libdir","--target","x86_64-pc-windows-msvc"],
        cwd=ROOT, encoding="utf-8").strip())
    if not target_dir.resolve().is_relative_to(sysroot.resolve()):
        raise ValueError("target library directory outside reviewed sysroot")
    inventory = target_library_inventory(target_dir, report["out_of_tree_dependencies"])
    print("RUST_WINDOWS_TARGET_ARCHIVES " + json.dumps(inventory,ensure_ascii=True,separators=(",",":")))
    print("RUST_LIBRARY_NOTICE_REVIEW " + json.dumps(report,ensure_ascii=True,separators=(",",":")))
    print("RUST_LIBRARY_NOTICE_REVIEW_PASS")


class Tests(unittest.TestCase):
    def test_target_inventory(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for name in ("std", "core", "alloc", "compiler_builtins", "hashbrown"):
                (directory / ("lib" + name + "-abcd.rlib")).write_bytes(b"fixture")
            rows = [{"crate":"hashbrown-0.17.1"}, {"crate":"fortanix-sgx-abi-0.6.1"}]
            result = target_library_inventory(directory, rows)
            self.assertEqual(result["matching_report_candidates"], ["hashbrown-0.17.1"])
            self.assertEqual(result["report_only_candidates"], ["fortanix-sgx-abi-0.6.1"])
            (directory / "libfortanix_sgx_abi-abcd.rlib").write_bytes(b"fixture")
            with self.assertRaisesRegex(ValueError,"SGX/UEFI"):
                target_library_inventory(directory, rows)

    def test_report_baseline(self):
        verify_report_baseline({"license":"MIT"},{"license":"MIT"})
        with self.assertRaises(ValueError):
            verify_report_baseline({"license":"BSD"},{"license":"MIT"})

    def test_template(self):
        fixture = """<h2 id="in-tree-files">In-tree files</h2>
        <div><p><b>License:</b> MIT OR Apache-2.0</p><p><b>Copyright:</b> fixture</p></div>
        <h2 id="out-of-tree-dependencies">Out-of-tree dependencies</h2>
        <h3>&#128230; test-crate-1.2.3</h3>
        <p><b>Authors:</b> fixture</p><p><b>License:</b> MIT</p>
        <p><b>Notices:</b><details><summary>LICENSE</summary><pre>original</pre></details></p>"""
        result = summarize(fixture.encode("utf-8"))
        self.assertEqual(result["in_tree_license_declarations"],{"MIT OR Apache-2.0":1})
        self.assertEqual(result["out_of_tree_dependencies"],[{"crate":"test-crate-1.2.3","license":"MIT","notice_blocks":1}])
        self.assertNotIn("fixture",json.dumps(result))
        with self.assertRaises(ValueError):
            summarize(fixture.replace("<p><b>License:</b> MIT</p>","").encode("utf-8"))
        with self.assertRaises(ValueError):
            summarize(b"<h2>unrelated</h2>")

if __name__ == "__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test",action="store_true")
    args=parser.parse_args()
    if args.self_test:
        if not unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests)).wasSuccessful():
            raise SystemExit(1)
    else:
        inspect()
