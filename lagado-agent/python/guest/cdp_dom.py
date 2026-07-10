"""cdp_dom.py — the DOM floor's guest-side reader: one CDP Runtime.evaluate over a minimal
stdlib WebSocket, returning the page's visible interactive elements as labeled screen-pixel
boxes (the web equivalent of an a11y-tree read).

Runs IN THE GUEST (deployed by perception/dom.rs, executed via the /execute channel) against
the local browser's DevTools endpoint. stdlib only — the guest has no websocket package.

Output: ONE JSON line on stdout:
    {"ok": true, "url": ..., "title": ..., "dpr": 1, "elements":
        [{"tag": "a", "role": "", "label": "Sign in", "x": 812, "y": 40, "w": 64, "h": 22}, ...]}
or  {"ok": false, "error": "..."}   (exit 1)

Coordinates are SCREEN pixels (viewport rect + window chrome offset), so boxes land in the
same space as a11y bboxes and CV proposals and fuse() can IoU-merge across senses.
"""
import base64
import hashlib
import json
import os
import socket
import struct
import sys
import urllib.request

# DevTools endpoint candidates: env override wins; else the two ports seen in the wild
# (9222 = the conventional default; 1337 = OSWorld's chrome task launch flag).
CDP_CANDIDATES = ([os.environ["LAGADO_CDP_HTTP"]] if os.environ.get("LAGADO_CDP_HTTP")
                  else ["http://127.0.0.1:9222", "http://127.0.0.1:1337"])
MAX_ELEMENTS = 400

# Everything interactive or semantically labeled, mapped to screen pixels. Element cap keeps the
# payload bounded on pathological pages; visibility filter drops zero-size/offscreen/hidden nodes.
WALKER_JS = r"""
(() => {
  const dpr = window.devicePixelRatio || 1;
  const bx = Math.max(0, (window.outerWidth - window.innerWidth) / 2);
  const by = Math.max(0, window.outerHeight - window.innerHeight - bx);
  const sx = window.screenX + bx, sy = window.screenY + by;
  const vis = (e, r) => r.width > 2 && r.height > 2 && r.bottom > 0 && r.right > 0 &&
      r.top < window.innerHeight && r.left < window.innerWidth &&
      getComputedStyle(e).visibility !== 'hidden' && getComputedStyle(e).display !== 'none';
  const label = e => (e.getAttribute('aria-label') || e.placeholder || e.value && String(e.value) ||
      e.innerText || e.title || e.alt || '').trim().replace(/\s+/g, ' ').slice(0, 80);
  const sel = 'a,button,input,select,textarea,summary,[role],[onclick],[contenteditable="true"]';
  const out = [];
  for (const e of document.querySelectorAll(sel)) {
    const r = e.getBoundingClientRect();
    if (!vis(e, r)) continue;
    out.push({tag: e.tagName.toLowerCase(), role: e.getAttribute('role') || '',
              label: label(e),
              x: Math.round(sx + r.x), y: Math.round(sy + r.y),
              w: Math.round(r.width), h: Math.round(r.height)});
    if (out.length >= __MAX__) break;
  }
  return JSON.stringify({url: location.href, title: document.title, dpr: dpr, elements: out});
})()
""".replace("__MAX__", str(MAX_ELEMENTS))


def fail(msg):
    print(json.dumps({"ok": False, "error": msg}), flush=True)
    raise SystemExit(1)


def ws_connect(ws_url, timeout=5):
    """Open a client WebSocket to ws://host:port/path (no TLS — DevTools is loopback-only)."""
    rest = ws_url.split("://", 1)[1]
    hostport, _, path = rest.partition("/")
    host, _, port = hostport.partition(":")
    s = socket.create_connection((host, int(port or 80)), timeout=timeout)
    s.settimeout(timeout)
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall(("GET /%s HTTP/1.1\r\nHost: %s\r\nUpgrade: websocket\r\n"
               "Connection: Upgrade\r\nSec-WebSocket-Key: %s\r\n"
               "Sec-WebSocket-Version: 13\r\n\r\n" % (path, hostport, key)).encode())
    resp = b""
    while b"\r\n\r\n" not in resp:
        chunk = s.recv(4096)
        if not chunk:
            fail("ws handshake: connection closed")
        resp += chunk
    if b" 101 " not in resp.split(b"\r\n", 1)[0]:
        fail("ws handshake refused: %r" % resp[:120])
    # RFC 6455 accept-key check — catches a non-websocket endpoint answering 101.
    want = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
                                         .encode()).digest())
    if want not in resp:
        fail("ws handshake: bad accept key")
    return s


def ws_send_text(s, text):
    payload = text.encode()
    mask = os.urandom(4)
    header = b"\x81"                       # FIN + text opcode
    n = len(payload)
    if n < 126:
        header += bytes([0x80 | n])
    elif n < (1 << 16):
        header += bytes([0x80 | 126]) + struct.pack(">H", n)
    else:
        header += bytes([0x80 | 127]) + struct.pack(">Q", n)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    s.sendall(header + mask + masked)


def _recv_exact(s, n):
    buf = b""
    while len(buf) < n:
        chunk = s.recv(n - len(buf))
        if not chunk:
            fail("ws recv: connection closed mid-frame")
        buf += chunk
    return buf


def ws_recv_text(s):
    """Receive one complete (possibly fragmented) text message; server frames are unmasked."""
    message = b""
    while True:
        b1, b2 = _recv_exact(s, 2)
        fin, opcode = b1 & 0x80, b1 & 0x0F
        n = b2 & 0x7F
        if n == 126:
            n = struct.unpack(">H", _recv_exact(s, 2))[0]
        elif n == 127:
            n = struct.unpack(">Q", _recv_exact(s, 8))[0]
        if b2 & 0x80:                      # masked server frame — protocol violation
            _recv_exact(s, 4)
        data = _recv_exact(s, n)
        if opcode == 0x8:                  # close
            fail("ws recv: server closed")
        if opcode == 0x9:                  # ping → pong, keep reading
            s.sendall(b"\x8a" + bytes([0x80]) + os.urandom(4))
            continue
        if opcode in (0x1, 0x0):           # text / continuation
            message += data
            if fin:
                return message.decode("utf-8", "replace")


def main():
    targets, errs = None, []
    for base in CDP_CANDIDATES:
        try:
            with urllib.request.urlopen(base + "/json", timeout=3) as r:
                targets = json.load(r)
            break
        except Exception as e:
            errs.append("%s: %r" % (base, e))
    if targets is None:
        fail("no DevTools endpoint (%s)" % "; ".join(errs))
    pages = [t for t in targets
             if t.get("type") == "page" and t.get("webSocketDebuggerUrl")
             and not t.get("url", "").startswith("devtools://")]
    if not pages:
        fail("no debuggable page target")
    ws = ws_connect(pages[0]["webSocketDebuggerUrl"])
    ws_send_text(ws, json.dumps({"id": 1, "method": "Runtime.evaluate",
                                 "params": {"expression": WALKER_JS, "returnByValue": True}}))
    for _ in range(50):                    # events may arrive before our reply
        msg = json.loads(ws_recv_text(ws))
        if msg.get("id") == 1:
            break
    else:
        fail("no reply to Runtime.evaluate")
    ws.close()
    result = (msg.get("result") or {}).get("result") or {}
    if result.get("type") != "string":
        fail("unexpected evaluate result: %r" % (msg,)[:200])
    payload = json.loads(result["value"])
    payload["ok"] = True
    print(json.dumps(payload), flush=True)


if __name__ == "__main__":
    main()
