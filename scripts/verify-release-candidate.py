"""Validate the exact candidate ZIP against inspected build evidence. Never publish."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
REPO = "fostergd/disc_archiving_app"
ZIP_NAME = "disc-archive-windows-x64-preview.zip"
PAYLOAD = {"disc-archive.exe", "disc-archive-studio.exe", "README.md",
           "WINDOWS_ACCEPTANCE.md", "LICENSE.md", "THIRD_PARTY_NOTICES.md",
           "THIRD_PARTY_LICENSES.zip"}
BINARIES = {"disc-archive.exe", "disc-archive-studio.exe"}
MAX_ZIP = 128*1024*1024


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def read_zip(raw, limit):
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        infos=archive.infolist()
        names=[i.filename for i in infos]
        if len(names)!=len({n.casefold() for n in names}) or len(names)>1000:
            raise ValueError("duplicate or excessive ZIP entries")
        if any(i.is_dir() or "\\" in i.filename or ":" in i.filename or
               any(part in ("",".","..") for part in i.filename.split("/"))
               for i in infos):
            raise ValueError("unsafe ZIP entry")
        if sum(i.file_size for i in infos)>limit:
            raise ValueError("expanded ZIP exceeds bound")
        return {i.filename:archive.read(i) for i in infos}


def verify_package(raw, sidecar, runtime, expected_docs):
    if sidecar.decode("ascii").strip()!=digest(raw)+"  "+ZIP_NAME:
        raise ValueError("external ZIP checksum mismatch")
    items=read_zip(raw,MAX_ZIP)
    if set(items)!=PAYLOAD|{"SHA256SUMS.txt"}:
        raise ValueError("candidate payload mismatch")
    checks=items["SHA256SUMS.txt"].decode("ascii").splitlines()
    expected={digest(items[name])+"  "+name for name in PAYLOAD}
    if len(checks)!=len(expected) or set(checks)!=expected:
        raise ValueError("candidate internal checksum mismatch")
    if runtime.get("distribution_approved") is not False:
        raise ValueError("unexpected runtime evidence approval")
    rows=runtime["binaries"]
    if len(rows)!=2 or {row["name"] for row in rows}!=BINARIES:
        raise ValueError("runtime evidence binary identity mismatch")
    for row in rows:
        data=items[row["name"]]
        if digest(data)!=row["sha256"] or len(data)!=row["size"] or row["machine"]!="x86_64":
            raise ValueError("candidate binary differs from inspected build")
    if set(expected_docs)!=PAYLOAD-BINARIES-{"THIRD_PARTY_LICENSES.zip"}:
        raise ValueError("expected document set mismatch")
    if any(items[n]!=data for n,data in expected_docs.items()):
        raise ValueError("candidate documents differ from source")
    notices=read_zip(items["THIRD_PARTY_LICENSES.zip"],MAX_ZIP)
    manifest=json.loads(notices.pop("MANIFEST.json"))
    if manifest!={"schema":1,"lock_sha256":runtime["lock_sha256"],
                  "distribution_approved":False,
                  "files":{name:digest(data) for name,data in notices.items()}}:
        raise ValueError("notice manifest or lock identity mismatch")
    if len(notices)!=348:
        raise ValueError("unexpected notice count")
    return {"zip_sha256":digest(raw),"zip_bytes":len(raw),
            "payload_sha256":{name:digest(items[name]) for name in sorted(PAYLOAD)},
            "notice_count":len(notices),"runtime_evidence_bound":True,
            "distribution_approved":False}


def validate_hold(hold):
    if (hold.get("schema")!=1 or hold.get("repository")!=REPO
            or hold.get("distribution_approved") is not False
            or not isinstance(hold.get("blockers"),list) or not hold["blockers"]
            or any(not isinstance(x,str) or not re.fullmatch(r"[a-z_]+",x) for x in hold["blockers"])):
        raise ValueError("invalid or changed publication hold; new gate implementation required")
    return hold["blockers"]


class Tests(unittest.TestCase):
    def fixture(self):
        lock="a"*64
        notices={f"n{i}.txt":b"notice" for i in range(348)}
        manifest={"schema":1,"lock_sha256":lock,"distribution_approved":False,
                  "files":{n:digest(b) for n,b in notices.items()}}
        def pack(items):
            stream=io.BytesIO()
            with zipfile.ZipFile(stream,"w") as z:
                for n,b in items.items():
                    z.writestr(n,b)
            return stream.getvalue()
        docs={n:b"fixture" for n in PAYLOAD-BINARIES-{"THIRD_PARTY_LICENSES.zip"}}
        items={**docs,**{n:b"MZfixture" for n in BINARIES},
               "THIRD_PARTY_LICENSES.zip":pack({**notices,"MANIFEST.json":json.dumps(manifest).encode()})}
        runtime={"lock_sha256":lock,"distribution_approved":False,
                 "binaries":[{"name":n,"size":len(items[n]),"sha256":digest(items[n]),"machine":"x86_64"} for n in BINARIES]}
        items["SHA256SUMS.txt"]=("\n".join(digest(b)+"  "+n for n,b in items.items())+"\n").encode()
        raw=pack(items)
        return raw,(digest(raw)+"  "+ZIP_NAME).encode(),runtime,docs

    def test_candidate(self):
        raw,sidecar,runtime,docs=self.fixture()
        report=verify_package(raw,sidecar,runtime,docs)
        self.assertTrue(report["runtime_evidence_bound"])
        self.assertFalse(report["distribution_approved"])
        with self.assertRaisesRegex(ValueError,"external ZIP"):
            verify_package(raw,b"bad",runtime,docs)
        runtime["binaries"][0]["sha256"]="0"*64
        with self.assertRaisesRegex(ValueError,"differs from inspected"):
            verify_package(raw,sidecar,runtime,docs)

    def test_hold(self):
        hold={"schema":1,"repository":REPO,"distribution_approved":False,"blockers":["review"]}
        self.assertEqual(validate_hold(hold),["review"])
        for change in ({"distribution_approved":True},{"blockers":[]},{"repository":"other/repo"}):
            with self.assertRaises(ValueError):
                validate_hold({**hold,**change})


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test",action="store_true")
    parser.add_argument("--directory",type=Path)
    parser.add_argument("--runtime-report",type=Path)
    parser.add_argument("--require-publication-approval",action="store_true")
    args=parser.parse_args()
    if args.self_test:
        if not unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests)).wasSuccessful():
            raise SystemExit(1)
    else:
        if not args.directory or not args.runtime_report:
            parser.error("candidate directory and runtime report required")
        zip_path=args.directory/ZIP_NAME
        sidecar=args.directory/(ZIP_NAME+".sha256")
        if zip_path.stat().st_size>MAX_ZIP or sidecar.stat().st_size>1024:
            raise ValueError("candidate asset exceeds bound")
        runtime=json.loads(args.runtime_report.read_text(encoding="utf-8"))
        docs={name:(ROOT/name).read_bytes() for name in PAYLOAD-BINARIES-{"THIRD_PARTY_LICENSES.zip"}}
        report=verify_package(zip_path.read_bytes(),sidecar.read_bytes(),runtime,docs)
        hold=json.loads((ROOT/"docs/compliance/public-release-hold.json").read_text(encoding="utf-8"))
        report["publication_blockers"]=validate_hold(hold)
        print("RELEASE_CANDIDATE_VERIFIED "+json.dumps(report,sort_keys=True,separators=(",",":")))
        print("PUBLICATION_BLOCKED")
        if args.require_publication_approval:
            raise SystemExit(1)
