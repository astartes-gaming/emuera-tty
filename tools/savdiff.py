#!/usr/bin/env python3
"""Compare two Emuera binary saves (save*.sav, global.sav, chara_*.dat).

    tools/savdiff.py EMUERA_MADE.sav OURS.sav

Decodes both (Emuera's "1808" format, gzip-compressed or not) and reports:
whether the payloads after the save description are byte-identical (the
goal: Emuera writes exactly these bytes), then any variable whose value
differs per character / globals / EM section, then where the order of
entries first differs. Order alone doesn't stop Emuera loading a save (it
reads by name) but byte identity needs it.

Round trip an Emuera-made save: in a scratch copy of the game add
ERB/ZZ_ROUNDTRIP.ERB containing
    @SYSTEM_LOADEND
    SAVEDATA 5, "roundtrip"
    QUIT
load the Emuera save from the title (headless: printf '1\n4\n' | …), then
compare it with save05.sav.
"""
import sys, gzip, struct

class R:
    def __init__(s, b): s.b = b; s.i = 0
    def u8(s): v = s.b[s.i]; s.i += 1; return v
    def raw(s, n): v = s.b[s.i:s.i+n]; s.i += n; return v
    def i64(s): return struct.unpack('<q', s.raw(8))[0]
    def i32(s): return struct.unpack('<i', s.raw(4))[0]
    def str(s):
        n = sh = 0
        while True:
            c = s.u8(); n |= (c & 0x7f) << sh; sh += 7
            if c < 0x80: break
        return s.raw(n).decode('utf-16-le')
    def cint(s, c=None):
        c = s.u8() if c is None else c
        if c <= 0xCF: return c
        if c == 0xD0: return struct.unpack('<h', s.raw(2))[0]
        if c == 0xD1: return s.i32()
        if c == 0xD2: return s.i64()
        raise ValueError(hex(c))
    def arr(s, dims, isstr):
        out = {}; total = 1
        for d in dims: total *= d
        idx = 0
        l = dims[1:]
        rowlen = 1
        for d in l: rowlen *= d
        planelen = (dims[2] if len(dims) == 3 else 1)
        # walk with positions (x,y,z)
        pos = [0, 0, 0]
        def flat():
            if len(dims) == 1: return pos[0]
            if len(dims) == 2: return pos[0] * dims[1] + pos[1]
            return (pos[0] * dims[1] + pos[1]) * dims[2] + pos[2]
        last = len(dims) - 1
        while True:
            c = s.u8()
            if c == 0xFF: return out
            if c == 0xF0: pos[last] += s.cint()
            elif c == 0xF1:
                n = s.cint()
                if len(dims) == 2: pos[0] += n; pos[1] = 0
                else: pos[1] += n; pos[2] = 0
            elif c == 0xF2: n = s.cint(); pos[0] += n; pos[1] = pos[2] = 0
            elif c == 0xE0:
                if len(dims) == 2: pos[0] += 1; pos[1] = 0
                else: pos[1] += 1; pos[2] = 0
            elif c == 0xE1: pos[0] += 1; pos[1] = pos[2] = 0
            elif c == 0xD8: out[flat()] = s.str(); pos[last] += 1
            else: out[flat()] = s.cint(c); pos[last] += 1
    def entries(s):
        """read entries until EOC/EOF; returns (dict, terminator, separators)"""
        d = {}; order = []
        while True:
            k = s.u8()
            if k in (0xFE, 0xFF): return d, order, k
            if k == 0xFD: order.append('|'); continue
            key = s.str(); order.append(key)
            if k == 0x00: d[key] = ('int', s.cint())
            elif k == 0x10: d[key] = ('str', s.str())
            elif k in (1, 2, 3, 0x11, 0x12, 0x13):
                nd = k & 0xF; dims = [s.i32() for _ in range(nd)]
                d[key] = (tuple(dims), s.arr(dims, k >= 0x10))
            elif k == 0x20:
                n = s.i32(); d[key] = ('map', tuple((s.str(), s.str()) for _ in range(n)))
            elif k == 0x21: d[key] = ('xml', s.str())
            elif k == 0x22: d[key] = ('dt', s.str(), s.str())
            else: raise ValueError(f"kind {k:#x} at {s.i}")
def load(path):
    b = open(path, 'rb').read(); body = b[16:]
    if b[:8] == bytes.fromhex('894552415a49500a'): body = gzip.decompress(body)
    r = R(body); ft = r.u8(); code = r.i64(); ver = r.i64(); text = r.str()
    res = {'type': ft, 'code': code, 'ver': ver, 'text': text}
    if ft == 0:
        n = r.i64(); res['charas'] = [r.entries() for _ in range(n)]
    res['globals'] = r.entries()
    res['em'] = r.entries() if r.i < len(body) else ({}, [], None)
    res['rest'] = len(body) - r.i
    return res


def payload(path):
    """The bytes after the header, file type, code, version and description."""
    b = open(path, "rb").read()
    body = b[16:]
    if b[:8] == bytes.fromhex("894552415a49500a"):
        body = gzip.decompress(body)
    r = R(body)
    r.u8(); r.i64(); r.i64(); text = r.str()
    return text, body[r.i:]


def cmp(tag, a, b):
    da, db = a[0], b[0]
    bad = 0
    for k in sorted(set(da) | set(db)):
        if da.get(k) != db.get(k):
            bad += 1
            if bad <= 8:
                va, vb = da.get(k), db.get(k)
                if va and vb and isinstance(va[1], dict) and isinstance(vb[1], dict):
                    diff = [(i, va[1].get(i), vb[1].get(i)) for i in sorted(set(va[1]) | set(vb[1])) if va[1].get(i) != vb[1].get(i)]
                    print(f"  {tag} {k}: dims {va[0]} vs {vb[0]}; {len(diff)} cells differ, e.g. {str(diff[:4])[:160]}")
                else:
                    print(f"  {tag} {k}: {str(va)[:100]} vs {str(vb)[:100]}")
    if bad > 8:
        print(f"  {tag}: ... {bad} differing keys")
    if a[2] != b[2]:
        print(f"  {tag}: terminator {a[2]} vs {b[2]}")
    return bad


def order(tag, a, b):
    oa, ob = a[1], b[1]
    if oa == ob:
        return 0
    k = next(i for i in range(min(len(oa), len(ob)) + 1) if i >= min(len(oa), len(ob)) or oa[i] != ob[i])
    print(f"  {tag}: order differs at entry {k} of {len(oa)}/{len(ob)}: {oa[k:k+4]} vs {ob[k:k+4]}")
    return 1


ta, pa = payload(sys.argv[1])
tb, pb = payload(sys.argv[2])
print("descriptions:", repr(ta), "|", repr(tb))
print("payload byte-identical:", pa == pb, f"({len(pa)} / {len(pb)} bytes)")
if pa != pb:
    A, B = load(sys.argv[1]), load(sys.argv[2])
    print("file type / game code / version same:", (A["type"], A["code"], A["ver"]) == (B["type"], B["code"], B["ver"]))
    bad = 0
    if "charas" in A:
        if len(A["charas"]) != len(B["charas"]):
            print("characters:", len(A["charas"]), "vs", len(B["charas"]))
        for i, (a, b) in enumerate(zip(A["charas"], B["charas"])):
            bad += cmp(f"chara{i}", a, b)
    bad += cmp("globals", A["globals"], B["globals"]) + cmp("em", A["em"], B["em"])
    print("values:", "all the same" if bad == 0 else f"{bad} variables differ")
    n = 0
    for i, (a, b) in enumerate(zip(A.get("charas", []), B.get("charas", []))):
        if n < 2:
            n += order(f"chara{i}", a, b)
    order("globals", A["globals"], B["globals"])
    order("em", A["em"], B["em"])
