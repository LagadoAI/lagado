"""uno_client.py — line-protocol RPC client for the resident UNO daemon.

Invoked once per agent step (in the guest, via the OSWorld `/execute` channel; on a
dev host, directly). It opens the daemon's Unix socket, sends ONE JSON request line,
reads ONE JSON response line, and prints it to stdout — so the caller parses the
`/execute` stdout exactly as it does for any other guest command.

Usage:
    uno_client.py <verb> [json-args] [--sock=PATH]

Examples:
    uno_client.py open '{"file": "/tmp/book.xlsx"}'
    uno_client.py apply '{"op": {"op": "fill", "sheet": "Sheet1", "range": "B1:E30", "direction": "down"}}'
    uno_client.py read '{"sheet": "Sheet1", "range": "B1:E30"}'
    uno_client.py structure
    uno_client.py reconcile
    uno_client.py close

Exit code: 0 if the response JSON has ok=true, else 1 (so a shell caller can branch).
"""

import json
import socket
import sys

DEFAULT_SOCK = "/tmp/lagado_uno_daemon.sock"


def call(sock_path, req, timeout=160.0):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(sock_path)
    f = s.makefile("rwb")
    f.write((json.dumps(req) + "\n").encode("utf-8"))
    f.flush()
    line = f.readline()
    s.close()
    if not line:
        return {"ok": False, "error": "empty response from daemon"}
    return json.loads(line.decode("utf-8"))


def main(argv):
    sock_path = DEFAULT_SOCK
    rest = []
    for a in argv:
        if a.startswith("--sock="):
            sock_path = a[len("--sock="):]
        else:
            rest.append(a)
    if not rest:
        sys.stderr.write("usage: uno_client.py <verb> [json-args] [--sock=PATH]\n")
        return 2
    verb = rest[0]
    args = json.loads(rest[1]) if len(rest) > 1 and rest[1].strip() else {}
    req = dict(args)
    req["verb"] = verb
    try:
        resp = call(sock_path, req)
    except Exception as e:
        resp = {"ok": False, "error": "client transport: %s: %s" % (type(e).__name__, e)}
    sys.stdout.write(json.dumps(resp) + "\n")
    return 0 if resp.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
