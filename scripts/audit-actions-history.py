"""Bounded read-only review of accessible Actions logs and artifact metadata.

Never emit log contents, artifact names, tokens, or redirect URLs.
This is a screening report, not a publication authorization.
"""
import argparse
from collections import Counter
import io
import json
import os
import re
import unittest
import urllib.error
import urllib.parse
import urllib.request
import zipfile

REPO = "fostergd/disc_archiving_app"
API = "https://api.github.com/repos/" + REPO + "/actions/"
MAX_RUNS = 150
MAX_ARTIFACTS = 500
MAX_DOWNLOAD = 16 * 1024 * 1024
MAX_TOTAL = 256 * 1024 * 1024
PATTERNS = {
    "private_key": re.compile(rb"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----"),
    "github_token": re.compile(rb"(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})"),
    "aws_key": re.compile(rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    "private_ip": re.compile(rb"(?<![0-9.])(?:192\.168|10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}(?![0-9.])"),
    "email": re.compile(rb"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request_bytes(url, limit, token=None):
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or parsed.username or parsed.password:
        raise ValueError("unexpected request URL")
    headers = {"User-Agent":"Disc-Archive-Studio-history-review"}
    if token:
        if parsed.netloc != "api.github.com" or not url.startswith(API):
            raise ValueError("refusing credential outside exact API scope")
        headers.update({"Authorization":"Bearer "+token,
                        "Accept":"application/vnd.github+json",
                        "X-GitHub-Api-Version":"2022-11-28"})
    req = urllib.request.Request(url,headers=headers)
    opener = urllib.request.build_opener(NoRedirect())
    try:
        with opener.open(req,timeout=30) as response:
            data = response.read(limit+1)
    except urllib.error.HTTPError as exc:
        if token and exc.code in (301,302,303,307,308):
            destination = exc.headers.get("Location","")
            exc.close()
            # The signed download URL is never logged or given credentials.
            return request_bytes(destination,limit)
        exc.close()
        raise
    if len(data)>limit:
        raise ValueError("download exceeds bound")
    return data


def api(path, token):
    return json.loads(request_bytes(API+path,MAX_DOWNLOAD,token))


def collect_pages(fetch, field, maximum):
    records = []
    expected = None
    for page in range(1, maximum//100+3):
        payload = fetch(page)
        total = payload["total_count"]
        if type(total) is not int or total<0:
            raise ValueError("invalid total count")
        if expected is None:
            expected = total
        elif expected != total:
            raise ValueError("inventory changed during pagination")
        if total>maximum:
            raise ValueError("inventory exceeds bound")
        records.extend(payload[field])
        if len(records)>=total:
            if len(records)!=total or len({x["id"] for x in records})!=total:
                raise ValueError("inventory count or identity mismatch")
            return records
        if not payload[field]:
            raise ValueError("incomplete page")
    raise ValueError("pagination bound reached")


def screen_zip(raw, remaining):
    counts = Counter({k:0 for k in PATTERNS})
    total = 0
    with zipfile.ZipFile(io.BytesIO(raw)) as z:
        members = z.infolist()
        if len(members)>1000 or len({m.filename for m in members})!=len(members):
            raise ValueError("unexpected log archive members")
        for member in members:
            if member.is_dir():
                continue
            if member.file_size>MAX_DOWNLOAD or total+member.file_size>remaining:
                raise ValueError("expanded log bound exceeded")
            data = z.read(member)
            if len(data)!=member.file_size:
                raise ValueError("log member size mismatch")
            total+=len(data)
            counts.update({k:len(v.findall(data)) for k,v in PATTERNS.items()})
    return total,dict(counts)


def inspect(token):
    report={"scope":"accessible Actions run-attempt logs and artifact metadata; counts only",
            "publication_approved":False,"runs":[],"artifacts":[],
            "artifact_inventory_complete":False,"log_inventory_complete":False}
    runs=collect_pages(lambda p:api("runs?per_page=100&page="+str(p),token),"workflow_runs",MAX_RUNS)
    report["listed_runs"]=len(runs)
    total=0
    for row in sorted(runs,key=lambda x:x["id"]):
        rid=row["id"]
        attempts=row["run_attempt"]
        if type(rid) is not int or type(attempts) is not int or not 1<=attempts<=10:
            raise ValueError("unexpected run identity")
        for attempt in range(1,attempts+1):
            item={"run_id":rid,"attempt":attempt}
            if row["status"]!="completed" and attempt==attempts:
                item["status"]="active_not_screened"
            else:
                try:
                    raw=request_bytes("https://api.github.com/repos/"+REPO+"/actions/runs/"+str(rid)+"/attempts/"+str(attempt)+"/logs",MAX_DOWNLOAD,token)
                    size,counts=screen_zip(raw,MAX_TOTAL-total)
                    total+=size
                    item.update(status="screened",expanded_bytes=size,pattern_counts=counts)
                except urllib.error.HTTPError as exc:
                    item.update(status="unavailable",http_status=exc.code)
                except (urllib.error.URLError,TimeoutError,ValueError,OSError,zipfile.BadZipFile):
                    item["status"]="unavailable_or_bound"
            report["runs"].append(item)
    report["expanded_bytes"]=total
    report["log_inventory_complete"]=all(x["status"]=="screened" for x in report["runs"])
    try:
        rows=collect_pages(lambda p:api("artifacts?per_page=100&page="+str(p),token),"artifacts",MAX_ARTIFACTS)
        for row in rows:
            run=row.get("workflow_run") or {}
            report["artifacts"].append({"id":row["id"],"expired":row["expired"],
                 "size_in_bytes":row["size_in_bytes"],"run_id":run.get("id")})
        report["artifact_inventory_complete"]=True
    except (urllib.error.URLError,TimeoutError,ValueError,KeyError,OSError):
        pass
    return report


class Tests(unittest.TestCase):
    def test_pagination(self):
        self.assertEqual(collect_pages(lambda p:{"total_count":1,"rows":[{"id":1}]},"rows",5),[{"id":1}])
        with self.assertRaises(ValueError):
            collect_pages(lambda p:{"total_count":2,"rows":[{"id":1},{"id":1}]},"rows",5)

    def test_screen(self):
        sample=b"fixture"+b"@example.invalid"
        stream=io.BytesIO()
        with zipfile.ZipFile(stream,"w") as z:
            z.writestr("fixture.log",sample)
        size,counts=screen_zip(stream.getvalue(),100)
        self.assertEqual(size,len(sample))
        self.assertEqual(counts["email"],1)
        with self.assertRaises(ValueError):
            screen_zip(stream.getvalue(),1)


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test",action="store_true")
    args=parser.parse_args()
    if args.self_test:
        if not unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests)).wasSuccessful():
            raise SystemExit(1)
    else:
        try:
            token=os.environ.get("GH_TOKEN")
            if not token or os.environ.get("GITHUB_REPOSITORY")!=REPO or os.environ.get("GITHUB_REF")!="refs/heads/main":
                raise ValueError("unexpected audit execution context")
            report=inspect(token)
            print("ACTIONS_HISTORY_REVIEW "+json.dumps(report,separators=(",",":"),sort_keys=True))
        except (urllib.error.URLError,TimeoutError,ValueError,KeyError,OSError):
            print("ACTIONS_HISTORY_REVIEW_INCOMPLETE: inventory unavailable or changed; publication remains blocked")
