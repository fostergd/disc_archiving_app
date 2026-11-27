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
