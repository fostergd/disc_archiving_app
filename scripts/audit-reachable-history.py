"""Read-only reachable Git history screening. Never log matching bytes or paths."""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
MAX_OBJECT = 32 * 1024 * 1024
MAX_TOTAL = 256 * 1024 * 1024
MAX_OBJECTS = 20000
# Exact independently retrieved upstream notice blobs; not a path-based exemption.
PUBLIC_UPSTREAM_NOTICE_BLOBS = {
    b"00ae6140bd54e1b538d833f60438e11ed8be12cf",
    b"0f65fd60fd68d465a2283378849f16532f5ee4f1",
    b"11069edd79019f7dafbe3138841cf289209270dd",
    b"139dce95d473c201a8561ef17536314d8b8cda65",
    b"16fe87b06e802f094b3fbb0894b137bca2b16ef1",
    b"1b5ec8b78e237b5c3b3d812a7c0a6589d0f7161d",
    b"36b7cd93cdfbac762f5be4c6ce276df2ea6305c2",
    b"4dad74f234eaaaa3c311ca031216e16fc9c4e940",
    b"52d82415d8b60cd3c5858c8939fa0d8fae1aeca0",
    b"673ea5f0505d6fa65c6a2b6a753c93b8e19d11b9",
    b"6a84e059ad8b43e03c2b06bc2af4dc1d8442808d",
    b"99016e120b21a6c77e3388d415968eaf6b504b4a",
    b"a2070c4be7acd5316f8d0357ca3c12419a2f3c62",
    b"ae78a8f94eae372cb1ca4e14b67244342fce604e",
    b"b0ec74488ab8a68acc95060414c7790a44a27172",
    b"d645695673349e3947e8e5ae42332d0ac3164cd7",
    b"d952d62c065f3f35fb83a173496e90b21525aef3",
}
HIGH = {
    "private_key_header": re.compile(rb"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----"),
    "github_token": re.compile(rb"(?<![A-Za-z0-9_])(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})(?![A-Za-z0-9_])"),
    "aws_access_key_id": re.compile(rb"(?<![A-Z0-9])(?:AKIA|ASIA)[A-Z0-9]{16}(?![A-Z0-9])"),
}
REVIEW = {
    "private_ipv4": re.compile(rb"(?<![0-9.])(?:10(?:\.\d{1,3}){3}|192\.168(?:\.\d{1,3}){2}|172\.(?:1[6-9]|2\d|3[01])(?:\.\d{1,3}){2})(?![0-9.])"),
    "local_user_path": re.compile(rb"(?i)(?:[a-z]:\\Users\\[^\\\r\n\"' ]+|/home/[^/\r\n\"' ]+/)"),
    "email_address": re.compile(rb"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
    "credential_assignment": re.compile(rb"(?i)\b(?:password|secret|api[_-]?key|access[_-]?token)\s*[:=]\s*[\"'][^\"'\r\n]{8,}[\"']"),
    "generic_api_token": re.compile(rb"(?<![A-Za-z0-9_-])sk-[A-Za-z0-9_-]{24,}(?![A-Za-z0-9_-])"),
}


def git(*args, data=None):
    # Suppress subprocess diagnostics so an unexpected path/value is not
    # inadvertently printed to a public workflow log.
    result = subprocess.run(["git", *args], cwd=ROOT, input=data,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise RuntimeError("Git history inspection command failed")
    return result.stdout


def classify(raw):
    return ({name for name, pattern in HIGH.items() if pattern.search(raw)},
            {name for name, pattern in REVIEW.items() if pattern.search(raw)})


def review_classes(raw, kind, oid):
    result = Counter()
    for m in REVIEW["email_address"].finditer(raw):
        if kind == "commit" and m.group().lower().endswith(b"@users.noreply.github.com"):
            start = raw.rfind(b"\n", 0, m.start()) + 1
            end = raw.find(b"\n", m.end())
            line = raw[start:end if end >= 0 else len(raw)]
            if start < raw.find(b"\n\n") and line.startswith((b"author ", b"committer ")):
                result["email_noreply_commit_header"] += 1
                continue
        if kind == "blob" and oid in PUBLIC_UPSTREAM_NOTICE_BLOBS:
            result["email_exact_public_upstream_notice"] += 1
        else:
            result["email_unclassified"] += 1
    for m in REVIEW["local_user_path"].finditer(raw):
        # The regex source itself contains a literal /home/ followed by
        # a negated character class. Classify only that exact fixture
        # match on the exact regex declaration, not arbitrary paths.
        start = raw.rfind(b"\n", 0, m.start()) + 1
        end = raw.find(b"\n", m.end())
        line = raw[start:end if end >= 0 else len(raw)]
        if (kind == "blob" and m.group() == b"/home/" + b"[^/"
                and line.strip().startswith(b'"local_user_path": re.compile(')
                and b"/home/" + b"[^/" in line):
            result["local_path_regex_literal"] += 1
        else:
            result["local_path_unclassified"] += 1
    return result


def inspect():
    if git("rev-parse", "--is-shallow-repository").strip() != b"false":
        raise RuntimeError("Full reachable history checkout is required")
    ids = git("rev-list", "--objects", "--all", "--no-object-names").splitlines()
    # Annotated tag objects may not be returned by all revision walks.
    ids += git("for-each-ref", "--format=%(objectname)", "refs/tags").splitlines()
    ids = sorted(set(ids))
    if not ids or len(ids) > MAX_OBJECTS or any(not re.fullmatch(rb"[0-9a-f]{40,64}", oid) for oid in ids):
        raise RuntimeError("Unexpected history object count or identity")
    info = git("cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)",
               data=b"\n".join(ids) + b"\n").splitlines()
    if len(info) != len(ids):
        raise RuntimeError("Incomplete Git object inventory")
    counts = Counter()
    high = Counter()
    review = Counter()
    review_details = Counter()
    skipped = Counter()
    total = 0
    for expected, row in zip(ids, info):
        parts = row.split()
        if len(parts) != 3 or parts[0] != expected:
            raise RuntimeError("Missing or mismatched Git object")
        kind = parts[1].decode("ascii")
        size = int(parts[2])
        counts[kind] += 1
        if kind not in ("blob", "commit", "tag", "tree"):
            raise RuntimeError("Unexpected Git object type")
        if size > MAX_OBJECT or total + size > MAX_TOTAL:
            skipped[kind] += 1
            continue
        # Raw trees contain names, and commit/tag objects include metadata
        # and messages. All are screened without logging matched content.
        raw = git("cat-file", kind, expected.decode("ascii"))
        if len(raw) != size:
            raise RuntimeError("Git object size mismatch")
        total += size
        a, b = classify(raw)
        high.update(a)
        review.update(b)
        review_details.update(review_classes(raw, kind, expected))
    metadata = git("log", "--all", "--format=%ae%x00%ce").splitlines()
    non_noreply = set()
    for line in metadata:
        for address in line.split(b"\0"):
            if address and not address.lower().endswith(b"@users.noreply.github.com"):
                non_noreply.add(address.lower())
    refs = git("for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes", "refs/tags").splitlines()
    return {
        "scope": "reachable objects from fetched local refs; heuristic screening only",
        "object_counts": dict(sorted(counts.items())),
        "ref_counts": {
            "heads": sum(x.startswith(b"refs/heads/") for x in refs),
            "remote_tracking": sum(x.startswith(b"refs/remotes/") for x in refs),
            "tags": sum(x.startswith(b"refs/tags/") for x in refs),
        },
        "scanned_bytes": total,
        "skipped_object_counts": dict(sorted(skipped.items())),
        "high_confidence_object_counts": dict(sorted(high.items())),
        "review_pattern_object_counts": dict(sorted(review.items())),
        "review_pattern_match_classification_counts": dict(sorted(review_details.items())),
        "distinct_non_noreply_commit_email_count": len(non_noreply),
        "complete_reachable_object_screen": not skipped,
        "publication_approved": False,
    }


class HistoryAuditTests(unittest.TestCase):
    def test_high_confidence(self):
        # Synthetic markers assembled at runtime, not committed as live-looking
        # credentials. The audit must never print any matched byte sequence.
        fake = b"gh" + b"p_" + b"Z" * 36
        self.assertEqual(classify(fake)[0], {"github_token"})
        key = b"-----BEGIN " + b"RSA PRIVATE " + b"KEY-----"
        self.assertEqual(classify(key)[0], {"private_key_header"})
        self.assertFalse(classify(b"ordinary project source")[0])

    def test_review_classification(self):
        email = b"fixture" + b"@users.noreply.github.com"
        raw = b"tree x\nauthor Test <" + email + b"> 0 +0000\n\nmessage " + email
        self.assertEqual(review_classes(raw, "commit", b"unknown"), {
            "email_noreply_commit_header": 1, "email_unclassified": 1})
        self.assertEqual(review_classes(email, "blob", next(iter(PUBLIC_UPSTREAM_NOTICE_BLOBS))),
                         {"email_exact_public_upstream_notice": 1})
        self.assertEqual(review_classes(email, "blob", b"unknown"), {"email_unclassified": 1})
        fixture = b'    "local_user_path": re.compile(rb"/home/' + b'[^/]+/"),\n'
        self.assertEqual(review_classes(fixture, "blob", b"unknown"), {"local_path_regex_literal": 1})
        self.assertEqual(review_classes(b"/home/" + b"fixture/", "blob", b"unknown"), {"local_path_unclassified": 1})

    def test_review_categories(self):
        raw = b"192" + b".168.42.7 " + b"fixture" + b"@example.invalid"
        self.assertEqual(classify(raw)[1], {"private_ipv4", "email_address"})
        self.assertFalse(classify(raw)[0])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        if not unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(HistoryAuditTests)).wasSuccessful():
            raise SystemExit(1)
    else:
        try:
            report = inspect()
        except (RuntimeError, ValueError, OSError):
            print("HISTORY_SCREEN_INCOMPLETE: object inspection failed; review required")
            raise SystemExit(1) from None
        print("HISTORY_SCREEN_COUNTS " + json.dumps(report, sort_keys=True))
        if not report["complete_reachable_object_screen"] or report["high_confidence_object_counts"]:
            print("HISTORY_SCREEN_REVIEW_REQUIRED: possible credential exposure or incomplete scope; do not publish")
            raise SystemExit(1)
        print("HISTORY_SCREEN_PASS: no high-confidence patterns in scanned reachable objects; manual review still required")
