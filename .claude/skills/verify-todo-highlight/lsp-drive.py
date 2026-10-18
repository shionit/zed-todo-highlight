#!/usr/bin/env python3
"""Drive todo-highlight-lsp over stdio the way Zed does, and record evidence.

Usage:
  lsp-drive.py doctor
  lsp-drive.py run <scenario> [--out DIR]
  lsp-drive.py list

Each run spawns its own server process, talks LSP to it, writes
transcript.jsonl (every frame, both directions), result.json (decoded tokens,
notifications, PASS/FAIL checks) and stderr.txt into the evidence dir, then
shuts the server down with shutdown/exit and kills only that child if needed.

Exit code: 0 = all checks passed, 1 = a check failed, 2 = harness error.
Stdlib only; works on the system python3 (3.9+).
"""
import json
import os
import queue
import re
import subprocess
import sys
import threading
import time

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
DEFAULT_BIN = os.path.join(REPO, "target", "release", "todo-highlight-lsp")
EVIDENCE_ROOT = os.path.join(REPO, "target", "verify-evidence")
TIMEOUT = 5.0

# Zed advertises textDocument.semanticTokens only when "semantic_tokens" is on.
CLIENT_CAPS_SEMANTIC = {
    "textDocument": {
        "semanticTokens": {
            "requests": {"full": True, "range": True},
            "tokenTypes": ["keyword"],
            "tokenModifiers": [],
            "formats": ["relative"],
        }
    }
}


class Session:
    def __init__(self, binary, out_dir, capabilities):
        self.out_dir = out_dir
        os.makedirs(out_dir, exist_ok=True)
        self.transcript = open(os.path.join(out_dir, "transcript.jsonl"), "w")
        self.stderr = open(os.path.join(out_dir, "stderr.txt"), "w")
        self.proc = subprocess.Popen(
            [binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr
        )
        self.inbox = queue.Queue()
        self.notifications = []
        self.next_id = 100
        threading.Thread(target=self._reader, daemon=True).start()
        self.init_result = self.request(
            "initialize", {"processId": os.getpid(), "rootUri": None, "capabilities": capabilities}
        )
        self.notify("initialized", {})
        self.legend = self.init_result["capabilities"]["semanticTokensProvider"]["legend"]

    def _log(self, direction, msg):
        self.transcript.write(json.dumps({"t": round(time.time(), 3), "dir": direction, "msg": msg}) + "\n")
        self.transcript.flush()

    def _reader(self):
        out = self.proc.stdout
        while True:
            length = None
            while True:
                line = out.readline()
                if not line:
                    self.inbox.put(None)
                    return
                line = line.strip()
                if not line:
                    break
                if line.lower().startswith(b"content-length:"):
                    length = int(line.split(b":")[1])
            msg = json.loads(out.read(length))
            self._log("server->client", msg)
            self.inbox.put(msg)

    def _send(self, msg):
        msg = dict(msg, jsonrpc="2.0")
        body = json.dumps(msg).encode()
        self._log("client->server", msg)
        self.proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.proc.stdin.flush()

    def _next(self, timeout):
        msg = self.inbox.get(timeout=timeout)
        if msg is None:
            raise RuntimeError("server closed stdout (crashed?) - see stderr.txt")
        return msg

    def _handle_unsolicited(self, msg):
        if "method" in msg and "id" in msg:
            # Server->client request (workspace/semanticTokens/refresh): answer like Zed.
            self._send({"id": msg["id"], "result": None})
        elif "method" in msg:
            self.notifications.append(msg)

    def request(self, method, params):
        rid = self.next_id
        self.next_id += 1
        self._send({"id": rid, "method": method, "params": params})
        deadline = time.time() + TIMEOUT
        while True:
            msg = self._next(max(0.01, deadline - time.time()))
            if msg.get("id") == rid and "method" not in msg:
                if "error" in msg:
                    raise RuntimeError("%s failed: %s" % (method, msg["error"]))
                return msg["result"]
            self._handle_unsolicited(msg)

    def notify(self, method, params):
        self._send({"method": method, "params": params})

    def settle(self, seconds=0.3):
        """Collect any notifications the server emits in the next `seconds`."""
        deadline = time.time() + seconds
        while time.time() < deadline:
            try:
                self._handle_unsolicited(self._next(deadline - time.time()))
            except queue.Empty:
                break

    def decode(self, data):
        """Delta-encoded semantic tokens -> [{line, col, len, type, modifiers}]."""
        out, line, col = [], 0, 0
        for i in range(0, len(data), 5):
            dl, dc, length, ttype, mods = data[i:i + 5]
            line += dl
            col = col + dc if dl == 0 else dc
            names = [m for b, m in enumerate(self.legend["tokenModifiers"]) if mods & (1 << b)]
            out.append({"line": line, "col": col, "len": length,
                        "type": self.legend["tokenTypes"][ttype], "modifiers": names})
        return out

    def close(self):
        try:
            if self.proc.poll() is None:
                self.request("shutdown", None)
                self.notify("exit", None)
                self.proc.wait(timeout=TIMEOUT)
        except Exception:
            pass
        finally:
            if self.proc.poll() is None:
                self.proc.kill()  # only our own child
                self.proc.wait()
            self.transcript.close()
            self.stderr.close()
        return self.proc.returncode


def open_doc(s, uri, text, version=1, lang="rust"):
    s.notify("textDocument/didOpen", {"textDocument": {
        "uri": uri, "languageId": lang, "version": version, "text": text}})


def full(s, uri):
    return s.decode(s.request("textDocument/semanticTokens/full", {"textDocument": {"uri": uri}})["data"])


def summarize(tokens, text):
    lines = text.split("\n")
    return [dict(t, text=lines[t["line"]][t["col"]:t["col"] + t["len"]]) for t in tokens]


def shown_messages(s):
    return [n["params"]["message"] for n in s.notifications if n["method"] == "window/showMessage"]


# --- scenarios: each returns (record, checks) where checks = [(name, bool)] ---

SAMPLE = (
    "// TODO: wire up config\n"
    "fn main() {}  // FIXME later, HACK around it\n"
    "/* NOTE INFO WARN WARNING BUG XXX DEPRECATED */\n"
    "// TODOS and MYTODO must not match\n"
)


def sc_highlight(s):
    uri = "file:///verify/sample.rs"
    open_doc(s, uri, SAMPLE)
    toks = summarize(full(s, uri), SAMPLE)
    words = [t["text"] for t in toks]
    expected = ["TODO", "FIXME", "HACK", "NOTE", "INFO", "WARN", "WARNING", "BUG", "XXX", "DEPRECATED"]
    return {"document": SAMPLE, "tokens": toks}, [
        ("all 10 keywords highlighted, in order", words == expected),
        ("every token is type 'keyword'", all(t["type"] == "keyword" for t in toks)),
        ("modifier == lowercased keyword", all(t["modifiers"] == [t["text"].lower()] for t in toks)),
        ("no match inside TODOS/MYTODO (line 3)", not any(t["line"] == 3 for t in toks)),
    ]


def sc_unicode(s):
    uri = "file:///verify/unicode.rs"
    text = "// 日本語 TODO 🎉 FIXME\n"
    open_doc(s, uri, text)
    toks = full(s, uri)
    # UTF-16 columns: '// ' =3, 日本語 =3, ' ' =1 -> TODO at 7; ' 🎉 ' = 1+2+1 -> FIXME at 15
    cols = [(t["col"], t["len"]) for t in toks]
    return {"document": text, "tokens": toks}, [
        ("columns are UTF-16 code units", cols == [(7, 4), (15, 5)]),
    ]


def sc_range(s):
    uri = "file:///verify/range.rs"
    open_doc(s, uri, SAMPLE)
    rng = {"start": {"line": 1, "character": 0}, "end": {"line": 2, "character": 0}}
    toks = s.decode(s.request("textDocument/semanticTokens/range",
                              {"textDocument": {"uri": uri}, "range": rng})["data"])
    toks = summarize(toks, SAMPLE)
    return {"range": rng, "tokens": toks}, [
        ("only line-1 keywords returned", [t["text"] for t in toks] == ["FIXME", "HACK"]),
        ("positions absolute from document origin", all(t["line"] == 1 for t in toks)),
    ]


def sc_edit(s):
    uri = "file:///verify/edit.rs"
    text = "let x = 1;\n// nothing here\n"
    open_doc(s, uri, text)
    before = full(s, uri)
    # Incremental change: type "TODO " at line 1, col 3 (after "// ").
    s.notify("textDocument/didChange", {
        "textDocument": {"uri": uri, "version": 2},
        "contentChanges": [{"range": {"start": {"line": 1, "character": 3},
                                      "end": {"line": 1, "character": 3}}, "text": "TODO "}]})
    after = full(s, uri)
    # Delete it again.
    s.notify("textDocument/didChange", {
        "textDocument": {"uri": uri, "version": 3},
        "contentChanges": [{"range": {"start": {"line": 1, "character": 3},
                                      "end": {"line": 1, "character": 8}}, "text": ""}]})
    reverted = full(s, uri)
    s.notify("textDocument/didClose", {"textDocument": {"uri": uri}})
    closed = full(s, uri)
    return {"before": before, "after_insert": after, "after_delete": reverted, "after_close": closed}, [
        ("no tokens before edit", before == []),
        ("TODO appears at 1:3 after insert",
         [(t["line"], t["col"], t["modifiers"]) for t in after] == [(1, 3, ["todo"])]),
        ("token gone after delete", reverted == []),
        ("closed document returns empty tokens", closed == []),
    ]


def sc_hint_off(s):
    """Zed with semantic tokens disabled: client never requests tokens."""
    open_doc(s, "file:///verify/a.rs", "// TODO a\n")
    open_doc(s, "file:///verify/b.rs", "// TODO b\n")
    s.settle()
    before = list(shown_messages(s))
    s.notify("textDocument/didClose", {"textDocument": {"uri": "file:///verify/a.rs"}})
    s.settle()
    s.notify("textDocument/didClose", {"textDocument": {"uri": "file:///verify/b.rs"}})
    s.settle()
    msgs = shown_messages(s)
    return {"shown_before_close": before, "shown_messages": msgs}, [
        ("no hint while only didOpens arrived", before == []),
        ("exactly one hint after didClose", len(msgs) == 1),
        ("hint points at semantic_tokens setting", bool(msgs) and '"semantic_tokens": "combined"' in msgs[0]),
    ]


def sc_hint_on(s):
    """Zed with semantic tokens enabled: tokens requested, so no hint ever."""
    for name in ("a", "b"):
        uri = "file:///verify/%s.rs" % name
        open_doc(s, uri, "// TODO %s\n" % name)
        full(s, uri)
        s.notify("textDocument/didClose", {"textDocument": {"uri": uri}})
    s.settle()
    msgs = shown_messages(s)
    return {"shown_messages": msgs}, [("no setup hint when tokens are requested", msgs == [])]


def sc_hint_unsupported(s):
    """Old Zed that never advertises semantic tokens: hint on first didClose."""
    open_doc(s, "file:///verify/a.rs", "// TODO a\n")
    s.notify("textDocument/didClose", {"textDocument": {"uri": "file:///verify/a.rs"}})
    s.settle()
    msgs = shown_messages(s)
    return {"shown_messages": msgs}, [
        ("exactly one hint", len(msgs) == 1),
        ("hint says update Zed", bool(msgs) and "update Zed" in msgs[0]),
    ]


def sc_startup(s):
    s.settle()
    logs = [n["params"]["message"] for n in s.notifications if n["method"] == "window/logMessage"]
    return {"init_result": s.init_result, "log_messages": logs}, [
        ("legend has only 'keyword' type", s.legend["tokenTypes"] == ["keyword"]),
        ("10 keyword modifiers in legend", len(s.legend["tokenModifiers"]) == 10),
        ("incremental sync advertised", s.init_result["capabilities"].get("textDocumentSync") == 2),
        ("range + full advertised", s.init_result["capabilities"]["semanticTokensProvider"].get("range") is True),
        ("version log line emitted", any(re.match(r"todo-highlight-lsp v\S+: initialized", m) for m in logs)),
    ]


# name -> (function, client advertises semantic tokens?)
SCENARIOS = {
    "startup": (sc_startup, True),
    "highlight": (sc_highlight, True),
    "unicode": (sc_unicode, True),
    "range": (sc_range, True),
    "edit": (sc_edit, True),
    "hint-off": (sc_hint_off, True),
    "hint-on": (sc_hint_on, True),
    "hint-unsupported": (sc_hint_unsupported, False),
}


def run_scenario(name, binary, out_dir):
    fn, semantic = SCENARIOS[name]
    s = Session(binary, out_dir, CLIENT_CAPS_SEMANTIC if semantic else {})
    record, checks, error = {}, [], None
    try:
        record, checks = fn(s)
    except Exception as e:  # recorded, not swallowed: shows up as FAIL
        error = "%s: %s" % (type(e).__name__, e)
    finally:
        code = s.close()
    passed = error is None and all(ok for _, ok in checks)
    result = {"scenario": name, "binary": binary, "passed": passed, "error": error,
              "server_exit_code": code,
              "checks": [{"name": n, "ok": ok} for n, ok in checks], "record": record}
    with open(os.path.join(out_dir, "result.json"), "w") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
    print("[%s] %s  -> %s" % ("PASS" if passed else "FAIL", name, out_dir))
    for n, ok in checks:
        print("   %s %s" % ("ok  " if ok else "FAIL", n))
    if error:
        print("   ERROR " + error)
    return passed


def doctor(binary):
    problems = []
    if not os.access(binary, os.X_OK):
        print("binary missing: %s\n  fix: cargo build --release -p todo-highlight-lsp" % binary)
        return False
    src_mtime = max(os.path.getmtime(os.path.join(dp, f))
                    for dp, _, fs in os.walk(os.path.join(REPO, "lsp", "src")) for f in fs)
    if os.path.getmtime(binary) < src_mtime:
        problems.append("binary is older than lsp/src - rebuild before trusting results")
    with open(os.path.join(REPO, "lsp", "Cargo.toml")) as f:
        want = re.search(r'^version\s*=\s*"([^"]+)"', f.read(), re.M).group(1)
    out_dir = os.path.join(EVIDENCE_ROOT, "doctor")
    s = Session(binary, out_dir, CLIENT_CAPS_SEMANTIC)
    s.settle()
    s.close()
    logs = [n["params"]["message"] for n in s.notifications if n["method"] == "window/logMessage"]
    got = next((m.split()[1].rstrip(":") for m in logs if "initialized" in m), None)
    if got != "v" + want:
        problems.append("server reports %s, lsp/Cargo.toml says v%s" % (got, want))
    print("binary:   %s" % binary)
    print("version:  %s (Cargo.toml v%s)" % (got, want))
    print("legend:   types=%s modifiers=%d" % (s.legend["tokenTypes"], len(s.legend["tokenModifiers"])))
    on_path = subprocess.run(["/usr/bin/which", "todo-highlight-lsp"], capture_output=True, text=True).stdout.strip()
    print("on PATH:  %s  (what a real Zed would launch; not used by this harness)" % (on_path or "none"))
    for p in problems:
        print("PROBLEM:  " + p)
    print("doctor: %s" % ("OK" if not problems else "NOT OK"))
    return not problems


def main(argv):
    binary = os.environ.get("TODO_LSP_BIN", DEFAULT_BIN)
    if len(argv) < 1 or argv[0] in ("-h", "--help"):
        print(__doc__)
        return 2
    if argv[0] == "list":
        print("\n".join(SCENARIOS))
        return 0
    if argv[0] == "doctor":
        return 0 if doctor(binary) else 1
    if argv[0] == "run" and len(argv) >= 2:
        names = list(SCENARIOS) if argv[1] == "all" else [argv[1]]
        unknown = [n for n in names if n not in SCENARIOS]
        if unknown:
            print("unknown scenario %s; try: %s" % (unknown, ", ".join(SCENARIOS)))
            return 2
        run_id = time.strftime("%Y%m%d-%H%M%S") + "-%d" % os.getpid()
        base = argv[argv.index("--out") + 1] if "--out" in argv else os.path.join(EVIDENCE_ROOT, run_id)
        ok = all([run_scenario(n, binary, os.path.join(base, n)) for n in names])
        return 0 if ok else 1
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
