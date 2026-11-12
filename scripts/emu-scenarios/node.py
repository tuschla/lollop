"""Query a uiautomator dump read on stdin (`dump` or `dump --windows`).

argv: [within ID] mode [text] [anchor]
  within ID        prefix: query only the subtree of the first node whose
                   resource-id ends with ':id/ID' (exit 1 when there is none).
  eq|pre|re TEXT   print "cx cy x1 y1 x2 y2 <label>" of the first node whose text or
                   content-desc equals / starts with / regex-matches TEXT (only
                   nodes AFTER the first node labelled exactly ANCHOR, when given);
                   exit 1 when absent.
  rid ID           same, matching the node's resource-id suffix (after ':id/' or exact).
  ridtext ID       print the text of that node.
  texts            print every label as "<key>=<value>" in document order.
  on TEXT          exit 0 when the node labelled exactly TEXT (or its parent,
                   grandparent or children) is checked/selected, 1 when not, 2 when
                   absent. The grandparent: a navigation rail item (landscape) wraps
                   its label one level deeper than a bottom-bar item.
  overflow         print labels whose text node's bounds exceed the screen width or
                   have zero/negative width (clipped/off-screen text), one per line.
  scroll           print "x1 y1 x2 y2" of the largest vertically scrollable node.
  expand TEXT      print "cx cy ..." of the collapsed-notification Expand button
                   (`expand_button`, content-desc "Expand") in the notification row
                   holding the first node whose label starts with TEXT; exit 1 when
                   the row is absent or already expanded.
"""
import re
import sys
import xml.etree.ElementTree as ET

args = sys.argv[1:]
scope = None
if args[:1] == ["within"]:
    scope, args = args[1], args[2:]
mode = args[0]
want = args[1] if len(args) > 1 else ""
anchor = args[2] if len(args) > 2 else None
raw = sys.stdin.read()
# `uiautomator dump --windows` wraps every window's <hierarchy> in <displays>.
i, j = raw.find("<?xml"), raw.rfind("</displays>")
j = j + 11 if j >= 0 else raw.rfind("</hierarchy>") + 12
if i < 0 or j < 12:
    sys.exit(2)
root = ET.fromstring(raw[i:j])
if scope is not None:
    root = next((n for n in root.iter("node") if n.get("resource-id", "").endswith(":id/" + scope)), None)
    if root is None:
        sys.exit(1)


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
            near = [n, parent.get(n), parent.get(parent.get(n))] + list(n)
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


if mode == "expand":
    parent = {c: p for p in root.iter() for c in p}
    hit = next((n for n in nodes for _, v in labels(n) if v.startswith(want)), None)
    p = parent.get(hit)
    while p is not None:
        btn = next((b for b in p.iter("node") if b.get("resource-id", "").endswith(":id/expand_button")), None)
        if btn is not None:
            if btn.get("content-desc") != "Expand":
                sys.exit(1)
            x1, y1, x2, y2 = bounds(btn)
            print((x1 + x2) // 2, (y1 + y2) // 2, x1, y1, x2, y2, "Expand")
            sys.exit(0)
        if p.get("resource-id", "").endswith(":id/expandableNotificationRow"):
            break
        p = parent.get(p)
    sys.exit(1)

seen = anchor is None
for n in nodes:
    if mode in ("rid", "ridtext"):
        rid = n.get("resource-id", "")
        if rid == want or rid.endswith(":id/" + want):
            x1, y1, x2, y2 = bounds(n)
            print(n.get("text", "") if mode == "ridtext" else f"{(x1 + x2) // 2} {(y1 + y2) // 2} {x1} {y1} {x2} {y2} {rid}")
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
