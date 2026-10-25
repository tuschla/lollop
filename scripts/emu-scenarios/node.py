"""Query a uiautomator dump read on stdin.

argv: mode [text] [anchor]
  eq|pre|re TEXT   print "cx cy x1 y1 x2 y2 <label>" of the first node whose text or
                   content-desc equals / starts with / regex-matches TEXT (only
                   nodes AFTER the first node labelled exactly ANCHOR, when given);
                   exit 1 when absent.
  rid ID           same, matching the node's resource-id suffix (after ':id/' or exact).
  texts            print every label as "<key>=<value>" in document order.
  on TEXT          exit 0 when the node labelled exactly TEXT (or its parent/children)
                   is checked/selected, 1 when not, 2 when absent.
  overflow         print labels whose text node's bounds exceed the screen width or
                   have zero/negative width (clipped/off-screen text), one per line.
  rtl              print "<n_left> <n_right>" counts of labelled nodes whose centre
                   lies in the left / right third of the root (layout mirroring probe).
  scroll           print "x1 y1 x2 y2" of the largest vertically scrollable node.
"""
import re
import sys
import xml.etree.ElementTree as ET

mode = sys.argv[1]
want = sys.argv[2] if len(sys.argv) > 2 else ""
anchor = sys.argv[3] if len(sys.argv) > 3 else None
raw = sys.stdin.read()
i, j = raw.find("<?xml"), raw.rfind("</hierarchy>")
if i < 0 or j < 0:
    sys.exit(2)
root = ET.fromstring(raw[i:j + 12])


def bounds(n):
    return tuple(map(int, re.findall(r"-?\d+", n.get("bounds", "[0,0][0,0]"))))


def labels(n):
    for k in ("text", "content-desc"):
        v = n.get(k, "")
        if v:
            yield k, v


if mode == "on":
    parent = {c: p for p in root.iter() for c in p}
    on = lambda n: n.get("checked") == "true" or n.get("selected") == "true"
    for n in root.iter("node"):
        if want in (n.get("text"), n.get("content-desc")):
            near = [n, parent.get(n)] + list(n)
            sys.exit(0 if any(m is not None and on(m) for m in near) else 1)
    sys.exit(2)

nodes = list(root.iter("node"))
W = max((bounds(n)[2] for n in nodes), default=0)

if mode == "overflow":
    for n in nodes:
        t = n.get("text", "")
        if not t:
            continue
        x1, y1, x2, y2 = bounds(n)
        if x2 <= x1 or x1 < 0 or x2 > W:
            print(f"{t!r} bounds={n.get('bounds')}")
    sys.exit(0)
if mode == "scroll":
    best = None
    for n in nodes:
        if n.get("scrollable") == "true":
            x1, y1, x2, y2 = bounds(n)
            if y2 - y1 > 80 and (best is None or (x2 - x1) * (y2 - y1) > (best[2] - best[0]) * (best[3] - best[1])):
                best = (x1, y1, x2, y2)
    if best is None:
        sys.exit(1)
    print(*best)
    sys.exit(0)


if mode == "rtl":
    left = right = 0
    for n in nodes:
        if not n.get("text"):
            continue
        x1, _, x2, _ = bounds(n)
        if x2 - x1 > W * 0.6:
            continue  # full-width rows say nothing about alignment
        c = (x1 + x2) / 2
        left += c < W / 3
        right += c > 2 * W / 3
    print(left, right)
    sys.exit(0)

seen = anchor is None
for n in nodes:
    if mode == "rid":
        rid = n.get("resource-id", "")
        if rid == want or rid.endswith(":id/" + want):
            x1, y1, x2, y2 = bounds(n)
            print((x1 + x2) // 2, (y1 + y2) // 2, x1, y1, x2, y2, rid)
            sys.exit(0)
        continue
    for k, v in labels(n):
        if not seen:
            seen = v == anchor
            continue
        if mode == "texts":
            print(f"{k}={v}")
            continue
        if (mode == "eq" and v == want) or (mode == "pre" and v.startswith(want)) \
                or (mode == "re" and re.search(want, v)):
            x1, y1, x2, y2 = bounds(n)
            print((x1 + x2) // 2, (y1 + y2) // 2, x1, y1, x2, y2, v)
            sys.exit(0)
sys.exit(0 if mode == "texts" else 1)
