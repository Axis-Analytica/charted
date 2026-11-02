#!/usr/bin/env python3
"""Record Power BI's answers through the Power BI Service instead of Desktop.

  bin/pbi login                                  once
  python3 conformance/service.py [--workspace NAME] [--only ID-PREFIX]

It publishes conformance/golden's semantic model to the workspace (default: My
workspace) as "charted-conformance", replacing it if it's there, refreshes it, runs
every query in queries.json through the executeQueries API (about 120 a minute, so
roughly 17 minutes) and writes conformance/expected.json in the same form as run.ps1.
"""
import argparse, base64, importlib.machinery, json, os, sys, time, urllib.error

sys.dont_write_bytecode = True

ROOT = os.path.dirname(os.path.abspath(__file__))
pbi = importlib.machinery.SourceFileLoader(
    "pbi", os.path.join(ROOT, "..", "bin", "pbi")
).load_module()
NAME = "charted-conformance"


def wait_operation(loc):
    """Polls a Fabric long-running operation until it finishes."""
    while True:
        time.sleep(3)
        _, _, op = pbi.request("fabric", "GET", loc)
        if op["status"] == "Succeeded":
            return
        if op["status"] in ("Failed", "Undefined"):
            sys.exit(f"publishing failed: {json.dumps(op)}")


def publish(ws):
    model = os.path.join(ROOT, "golden", "golden.SemanticModel")
    parts = []
    for d, _, files in os.walk(model):
        for name in sorted(files):
            full = os.path.join(d, name)
            rel = os.path.relpath(full, model).replace(os.sep, "/")
            if rel == ".platform":
                continue
            with open(full, "rb") as f:
                payload = base64.b64encode(f.read()).decode()
            parts.append({"path": rel, "payload": payload, "payloadType": "InlineBase64"})
    definition = {"definition": {"parts": parts}}
    _, _, items = pbi.request("fabric", "GET", f"workspaces/{ws}/semanticModels")
    existing = next((m for m in items["value"] if m["displayName"] == NAME), None)
    if existing:
        path = f"workspaces/{ws}/semanticModels/{existing['id']}/updateDefinition"
        status, loc, _ = pbi.request("fabric", "POST", path, definition)
    else:
        status, loc, _ = pbi.request(
            "fabric", "POST", f"workspaces/{ws}/semanticModels", {"displayName": NAME, **definition}
        )
    if status == 202:
        wait_operation(loc)
    _, _, items = pbi.request("fabric", "GET", f"workspaces/{ws}/semanticModels")
    return next(m["id"] for m in items["value"] if m["displayName"] == NAME)


def refresh(group, ds):
    pbi.request("powerbi", "POST", f"{group}datasets/{ds}/refreshes", {"notifyOption": "NoNotification"})
    while True:
        time.sleep(5)
        _, _, h = pbi.request("powerbi", "GET", f"{group}datasets/{ds}/refreshes?$top=1")
        r = h["value"][0]
        if r["status"] == "Completed":
            return
        if r["status"] == "Failed":
            sys.exit(f"refresh failed: {r.get('serviceExceptionJson', r)}")


def cell(v):
    if isinstance(v, str) and v in ("Infinity", "-Infinity", "NaN"):
        return {"num": v}
    return v


def run(group, ds, dax):
    """Rows as lists in column order, or {"error": message}."""
    body = {"queries": [{"query": dax}], "serializerSettings": {"includeNulls": True}}
    for attempt in range(8):
        try:
            _, _, doc = pbi.request("powerbi", "POST", f"{group}datasets/{ds}/executeQueries", body)
        except urllib.error.HTTPError as e:
            text = e.read().decode("utf-8", "replace")
            if e.code == 429 or e.code >= 500:
                time.sleep(int(e.headers.get("Retry-After") or 10 * (attempt + 1)))
                continue
            try:
                err = json.loads(text)["error"]
                details = err.get("pbi.error", {}).get("details", [])
                msg = next(
                    (d["detail"]["value"] for d in details if d.get("code") == "DetailsMessage"),
                    err.get("message", text),
                )
            except (ValueError, KeyError, TypeError):
                msg = text
            return {"error": msg}
        except (urllib.error.URLError, ConnectionError, TimeoutError) as e:
            print(f"network error ({e}); retrying", flush=True)
            time.sleep(10 * (attempt + 1))
            continue
        res = doc["results"][0]
        if "error" in res:
            return {"error": json.dumps(res["error"])}
        rows = res["tables"][0].get("rows", [])
        return {"rows": [[cell(v) for v in r.values()] for r in rows]}
    sys.exit("the service kept refusing queries")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workspace", help="workspace name (default: My workspace)")
    ap.add_argument("--only", help="run only queries whose id starts with this")
    ap.add_argument("--no-publish", action="store_true")
    args = ap.parse_args()

    if args.workspace:
        _, _, ws = pbi.request("fabric", "GET", "workspaces")
        ws_id = next(w["id"] for w in ws["value"] if w["displayName"] == args.workspace)
        group = f"groups/{ws_id}/"
    else:
        _, _, ws = pbi.request("fabric", "GET", "workspaces")
        ws_id = next(w["id"] for w in ws["value"] if w["type"] == "Personal")
        group = ""
    if args.no_publish:
        _, _, items = pbi.request("fabric", "GET", f"workspaces/{ws_id}/semanticModels")
        ds = next(m["id"] for m in items["value"] if m["displayName"] == NAME)
    else:
        print("publishing…", flush=True)
        ds = publish(ws_id)
        print("refreshing…", flush=True)
        refresh(group, ds)

    with open(os.path.join(ROOT, "queries.json"), encoding="utf-8") as f:
        doc = json.load(f)
    queries = [q for q in doc["queries"] if not args.only or q["id"].startswith(args.only)]
    path = os.path.join(ROOT, "expected.partial.json" if args.only else "expected.json")
    progress = path + ".progress"
    # Answers already recorded for the same fixture and query text are kept, so an
    # interrupted run picks up where it stopped (and a re-run only asks what changed).
    results = {}
    for old in (progress, path):
        try:
            with open(old, encoding="utf-8") as f:
                prev = json.load(f)
        except (OSError, ValueError):
            continue
        if prev.get("fixture") == doc["fixture"]:
            results.update({k: v for k, v in prev["results"].items() if k not in results})
    want = {q["id"]: q["hash"] for q in queries}
    results = {k: v for k, v in results.items() if want.get(k) == v.get("hash")}
    todo = [q for q in queries if q["id"] not in results]
    if not args.no_publish:
        todo, results = queries, {}
    print(f"{len(todo)} queries to run, {len(results)} already recorded", flush=True)

    def write(target):
        out = {"fixture": doc["fixture"], "powerbi": "Power BI Service", "results": results}
        with open(target, "w", encoding="utf-8") as f:
            json.dump(out, f, indent=1, ensure_ascii=False, sort_keys=True)
            f.write("\n")

    started = time.time()
    for i, q in enumerate(todo):
        # executeQueries allows 120 queries a minute per user.
        time.sleep(max(0, started + i * 0.51 - time.time()))
        results[q["id"]] = {"hash": q["hash"], **run(group, ds, q["dax"])}
        if (i + 1) % 100 == 0:
            print(f"{i + 1}/{len(todo)}", flush=True)
            write(progress)
    write(path)
    if os.path.exists(progress):
        os.remove(progress)
    errors = sum("error" in r for r in results.values())
    print(f"wrote {path}: {len(results)} results, {errors} errors")


if __name__ == "__main__":
    main()
