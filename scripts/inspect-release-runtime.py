"""Review-only compilation and PE import evidence for the pinned Windows release."""
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
LOCK = "60862ef1ad2cfab8812323835ad07f16294ccd4b09b6a2b32b7ea5dc5cccc567"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def pe_imports(raw):
    def unpack(fmt, off):
        if off < 0 or off + struct.calcsize(fmt) > len(raw):
            raise ValueError("PE field outside file")
        return struct.unpack_from(fmt, raw, off)
    if raw[:2] != b"MZ":
        raise ValueError("missing DOS header")
    pe, = unpack("<I", 60)
    if raw[pe:pe+4] != b"PE\0\0":
        raise ValueError("missing PE header")
    machine, sections = unpack("<HH", pe+4)
    optional_size, = unpack("<H", pe+20)
    opt = pe+24
    magic, = unpack("<H", opt)
    if machine != 0x8664 or magic != 0x20b or optional_size < 112:
        raise ValueError("expected Windows x64 PE32+")
    image_base, = unpack("<Q", opt+24)
    directories, = unpack("<I", opt+108)
    if directories > (optional_size-112)//8:
        raise ValueError("PE data directories exceed optional header")
    ranges = []
    for i in range(sections):
        off = opt+optional_size+40*i
        vsize, va, size, ptr = unpack("<IIII", off+8)
        if ptr+size > len(raw):
            raise ValueError("PE section outside file")
        ranges.append((va, size, ptr))

    def rva(addr, count=1):
        matches = [ptr+addr-va for va,size,ptr in ranges
                   if va <= addr and addr+count <= va+size]
        if len(matches) != 1:
            raise ValueError("unmapped or ambiguous PE RVA")
        return matches[0]

    def name(addr):
        start = rva(addr)
        end = raw.find(b"\0", start, min(len(raw), start+512))
        if end < 0:
            raise ValueError("unterminated import name")
        text = raw[start:end].decode("ascii")
        if not re.fullmatch(r"[A-Za-z0-9_.-]+\.dll", text, re.I):
            raise ValueError("unexpected DLL name")
        return text.lower()

    def table(index, stride, delay=False):
        if directories <= index:
            return []
        addr, size = unpack("<II", opt+112+index*8)
        if not addr and not size:
            return []
        if not addr or size < stride or size > 16*1024*1024:
            raise ValueError("invalid import table")
        found = set()
        for i in range(size//stride):
            off = rva(addr+i*stride, stride)
            words = unpack("<" + "I"*(stride//4), off)
            if not any(words):
                return sorted(found)
            if delay:
                attrs, dll = words[:2]
                if attrs not in (0, 1):
                    raise ValueError("unknown delay import attributes")
                if attrs == 0:
                    dll -= image_base
            else:
                dll = words[3]
            found.add(name(dll))
        raise ValueError("import table terminator absent")
    return {"machine": "x86_64", "imports": table(1,20), "delay_imports": table(13,32,True)}


def read_events(path, packages, root_id):
    finished = []
    found = {}
    executables = {}
    with path.open(encoding="utf-8-sig") as stream:
        for line in stream:
            if not line.strip():
                continue
            e = json.loads(line)
            if e.get("reason") == "build-finished":
                finished.append(e.get("success"))
            if e.get("reason") != "compiler-artifact":
                continue
            pid = e["package_id"]
            if pid not in packages:
                raise ValueError("compiled package not in locked Windows metadata")
            target = e["target"]
            p = packages[pid]
            key = f"{p['name']}@{p['version']}"
            entry = found.setdefault(key, {"name":p["name"],"version":p["version"],
                                         "license":p["license"],"target_kinds":set()})
            entry["target_kinds"].update(target["kind"])
            if pid == root_id and e.get("executable") and "bin" in target["kind"]:
                executable = Path(e["executable"])
                if target["name"] in executables and executable != executables[target["name"]]:
                    raise ValueError("ambiguous release executable")
                executables[target["name"]] = executable
    if finished != [True]:
        raise ValueError("not one successful build")
    if set(executables) != {"disc-archive", "disc-archive-studio"}:
        raise ValueError("unexpected root executable set")
    for row in found.values():
        row["target_kinds"] = sorted(row["target_kinds"])
    return [found[k] for k in sorted(found)], executables


def inspect(messages, output):
    if output.exists():
        raise ValueError("runtime evidence output already exists")
    if digest((ROOT/"Cargo.lock").read_bytes()) != LOCK:
        raise ValueError("reviewed lockfile changed")
    meta = json.loads(subprocess.check_output(
        ["cargo","metadata","--locked","--format-version","1",
         "--filter-platform","x86_64-pc-windows-msvc","--features","gui"],
        cwd=ROOT,encoding="utf-8"))
    root_id = meta["resolve"]["root"]
    nodes = {n["id"] for n in meta["resolve"]["nodes"]}
    packages = {p["id"]:p for p in meta["packages"] if p["id"] in nodes}
    compiled, executables = read_events(messages, packages, root_id)
    target = Path(meta["target_directory"]).resolve()
    binaries = []
    for name, path in sorted(executables.items()):
        if not path.is_absolute() or not path.resolve().is_relative_to(target) or path.name != name+".exe":
            raise ValueError("unexpected executable path")
        raw = path.read_bytes()
        binaries.append({"name":path.name,"sha256":digest(raw),"size":len(raw),**pe_imports(raw)})
    inventory = json.loads((ROOT/"docs/compliance/windows-license-inventory.json").read_text(encoding="utf-8"))
    candidates = {(p["name"],p["version"]) for p in inventory["packages"]}
    actual = {(p["name"],p["version"]) for p in compiled if p["name"] != packages[root_id]["name"]}
    if not actual <= candidates:
        raise ValueError("compiled dependency outside reviewed inventory")
    rustc = subprocess.check_output(["rustc","--version"],cwd=ROOT,encoding="utf-8").strip()
    report = {"scope":"compiler artifacts and direct PE import evidence, not a linked SBOM or license approval",
              "lock_sha256":LOCK,"toolchain":rustc,"compiled_packages":compiled,
              "candidate_only_packages":[{"name":n,"version":v} for n,v in sorted(candidates-actual)],
              "binaries":binaries,"distribution_approved":False}
    output.write_text(json.dumps(report,indent=2,ensure_ascii=True)+"\n",encoding="utf-8")
    print("RUNTIME_EVIDENCE_BEGIN")
    print(json.dumps(report,ensure_ascii=True,separators=(",",":")))
    print("RUNTIME_EVIDENCE_END")
    print(f"RUNTIME_EVIDENCE_PASS compiled_candidates={len(actual)} candidate_only={len(candidates-actual)} binaries={len(binaries)}")


class Tests(unittest.TestCase):
    def test_pe_imports(self):
        raw = bytearray(1024)
        raw[:2] = b"MZ"
        struct.pack_into("<I",raw,60,64)
        raw[64:68] = b"PE\0\0"
        struct.pack_into("<HH",raw,68,0x8664,1)
        struct.pack_into("<H",raw,84,240)
        opt=88
        struct.pack_into("<H",raw,opt,0x20b)
        struct.pack_into("<Q",raw,opt+24,0x140000000)
        struct.pack_into("<I",raw,opt+108,16)
        struct.pack_into("<II",raw,opt+120,0x1000,40)
        struct.pack_into("<II",raw,opt+112+13*8,0x1040,64)
        struct.pack_into("<IIII",raw,opt+240+8,512,0x1000,512,512)
        struct.pack_into("<IIIII",raw,512,1,0,0,0x1100,1)
        struct.pack_into("<II",raw,576,1,0x1120)
        raw[768:781]=b"KERNEL32.dll\0"
        raw[800:811]=b"USER32.dll\0"
        self.assertEqual(pe_imports(raw),{"machine":"x86_64","imports":["kernel32.dll"],"delay_imports":["user32.dll"]})
        with self.assertRaises(ValueError):
            pe_imports(raw[:600])

    def test_events(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)/"events"
            events=[{"reason":"compiler-artifact","package_id":"root","target":{"name":n,"kind":["bin"]},"executable":n+".exe"} for n in ("disc-archive","disc-archive-studio")]
            events.append({"reason":"build-finished","success":True})
            p.write_text("\n".join(json.dumps(e) for e in events),encoding="utf-8")
            rows,bins=read_events(p,{"root":{"name":"root","version":"1","license":None}},"root")
            self.assertEqual(len(rows),1)
            self.assertEqual(len(bins),2)
            p.write_text(json.dumps({"reason":"build-finished","success":False}),encoding="utf-8")
            with self.assertRaises(ValueError):
                read_events(p,{},"root")


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test",action="store_true")
    parser.add_argument("--build-messages",type=Path)
    parser.add_argument("--output",type=Path)
    args=parser.parse_args()
    if args.self_test:
        if not unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests)).wasSuccessful():
            raise SystemExit(1)
    elif args.build_messages and args.output:
        inspect(args.build_messages,args.output)
    else:
        parser.error("specify --self-test or build messages and output")
