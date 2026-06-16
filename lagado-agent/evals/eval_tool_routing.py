#!/usr/bin/env python3
"""
eval_tool_routing.py — Tool-routing compounding eval (Phase 0 / G-eval baseline)

Measures single-turn vs multi-turn classifier accuracy on the LFM2.5-1.2B-Instruct
model running on :8081. This turns the borrowed 0.63^5 ≈ 10% figure into a number
we actually own from our real tool schema + our real model checkpoint.

Run FIRST: start the classifier server:
  ~/.laputa-secure/../projects/lagado/lagado-agent/vendored/llama.cpp-2/build/bin/llama-server \\
    --model ~/.laputa-secure/models/LFM2.5-1.2B-Instruct-Q4_K_M.gguf \\
    --port 8081 --host 127.0.0.1 --ctx-size 512 --n-predict 10

Usage: python3 evals/eval_tool_routing.py [--port 8081] [--turns 5]
"""

import argparse
import json
import sys
import urllib.request
import urllib.error

SYSTEM_PROMPT = """\
Classify each message as CHAT, INTERACTIVE, or REASONING. One word only.

Examples:
open Firefox → INTERACTIVE
hello friend → CHAT
write sorting code → REASONING
click submit button → INTERACTIVE
explain how TCP works → REASONING
what time is it → CHAT
navigate to settings → INTERACTIVE
type my password → INTERACTIVE
search for files → INTERACTIVE
close this window → INTERACTIVE
how are you today → CHAT

Now classify:
{message} →"""

# Ground-truth labelled test cases: (message, expected_label)
TEST_CASES = [
    # INTERACTIVE — desktop actions
    ("open Firefox",                    "INTERACTIVE"),
    ("click the submit button",         "INTERACTIVE"),
    ("type hello world into the box",   "INTERACTIVE"),
    ("close this window",               "INTERACTIVE"),
    ("navigate to settings",            "INTERACTIVE"),
    ("search for my files",             "INTERACTIVE"),
    ("right click on the desktop",      "INTERACTIVE"),
    ("drag the file to downloads",      "INTERACTIVE"),
    ("scroll down on the page",         "INTERACTIVE"),
    ("press Escape",                    "INTERACTIVE"),
    # CHAT — conversational
    ("hello friend",                    "CHAT"),
    ("how are you today",               "CHAT"),
    ("what time is it",                 "CHAT"),
    ("thanks for your help",            "CHAT"),
    ("what can you do",                 "CHAT"),
    # REASONING — analytical / generative
    ("write sorting code in Python",    "REASONING"),
    ("explain how TCP works",           "REASONING"),
    ("summarize this document",         "REASONING"),
    ("what is the best approach here",  "REASONING"),
    ("compare these two options",       "REASONING"),
]


def classify_single(base_url: str, message: str) -> str:
    prompt = SYSTEM_PROMPT.format(message=message)
    body = json.dumps({
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.0,
        "top_k": 50,
        "repeat_penalty": 1.05,
        "max_tokens": 10,
        "stream": False,
    }).encode()
    req = urllib.request.Request(
        f"{base_url}/v1/chat/completions",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read())
        text = data["choices"][0]["message"]["content"].strip().upper()
        if "INTERACTIVE" in text: return "INTERACTIVE"
        if "REASONING"   in text: return "REASONING"
        return "CHAT"
    except Exception as e:
        print(f"  [error] {e}", file=sys.stderr)
        return "ERROR"


def classify_multi_turn(base_url: str, message: str, prior_turns: int) -> str:
    """
    Simulate multi-turn degradation: prepend N synthetic prior interactions
    to the conversation history before classifying the target message.
    This is the contaminated version — mimicking what happens when an agent
    keeps appending history instead of resetting to single-turn-fresh context.
    """
    messages = []
    # Inject synthetic prior turns (unrelated noise, as would accumulate in a real session)
    noise = [
        ("open terminal",              "INTERACTIVE"),
        ("type ls -la",                "INTERACTIVE"),
        ("explain what ls does",       "REASONING"),
        ("what time is it",            "CHAT"),
        ("click the back button",      "INTERACTIVE"),
    ]
    for i, (msg, label) in enumerate(noise[:prior_turns]):
        messages.append({"role": "user",      "content": SYSTEM_PROMPT.format(message=msg)})
        messages.append({"role": "assistant", "content": label})
    messages.append({"role": "user", "content": SYSTEM_PROMPT.format(message=message)})

    body = json.dumps({
        "messages": messages,
        "temperature": 0.0,
        "top_k": 50,
        "repeat_penalty": 1.05,
        "max_tokens": 10,
        "stream": False,
    }).encode()
    req = urllib.request.Request(
        f"{base_url}/v1/chat/completions",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read())
        text = data["choices"][0]["message"]["content"].strip().upper()
        if "INTERACTIVE" in text: return "INTERACTIVE"
        if "REASONING"   in text: return "REASONING"
        return "CHAT"
    except Exception as e:
        print(f"  [error] {e}", file=sys.stderr)
        return "ERROR"


def run_eval(base_url: str, max_turns: int):
    print(f"Tool-routing compounding eval — {base_url}")
    print(f"Cases: {len(TEST_CASES)}   Max turns: {max_turns}")
    print()

    # Single-turn baseline
    print("=== Single-turn baseline ===")
    correct = 0
    results = []
    for msg, expected in TEST_CASES:
        got = classify_single(base_url, msg)
        ok = got == expected
        correct += ok
        results.append((msg, expected, got, ok))
        status = "✓" if ok else "✗"
        print(f"  {status}  [{expected:>11}] ← '{msg}'  (got: {got})")
    single_acc = correct / len(TEST_CASES)
    print(f"\nSingle-turn accuracy: {correct}/{len(TEST_CASES)} = {single_acc:.1%}")
    print()

    # Multi-turn degradation
    print("=== Multi-turn degradation ===")
    turn_accs = [single_acc]
    for t in range(1, max_turns + 1):
        correct_t = 0
        for msg, expected in TEST_CASES:
            got = classify_multi_turn(base_url, msg, prior_turns=t)
            correct_t += got == expected
        acc_t = correct_t / len(TEST_CASES)
        turn_accs.append(acc_t)
        compound_expected = single_acc ** (t + 1)
        print(f"  Turn {t}: {acc_t:.1%}  (naive compound model predicts {compound_expected:.1%})")

    print()
    print("=== Summary ===")
    print(f"  Turn 0 (single): {turn_accs[0]:.1%}")
    for i, acc in enumerate(turn_accs[1:], 1):
        print(f"  Turn {i}:          {acc:.1%}")
    observed_drop = turn_accs[0] - turn_accs[-1]
    print(f"  Drop over {max_turns} turns: {observed_drop:.1%}")
    print()
    print("Save these numbers. They calibrate how hard the Board must work to")
    print("keep every classifier call single-turn-fresh.")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--port",  type=int, default=8081)
    p.add_argument("--turns", type=int, default=5)
    args = p.parse_args()
    base_url = f"http://127.0.0.1:{args.port}"

    # Health check
    try:
        with urllib.request.urlopen(f"{base_url}/health", timeout=3):
            pass
    except Exception:
        print(f"ERROR: classifier server not reachable at {base_url}", file=sys.stderr)
        print("Start it with:", file=sys.stderr)
        print("  llama-server --model ~/.laputa-secure/models/LFM2.5-1.2B-Instruct-Q4_K_M.gguf \\", file=sys.stderr)
        print("    --port 8081 --host 127.0.0.1 --ctx-size 512", file=sys.stderr)
        sys.exit(1)

    run_eval(base_url, args.turns)


if __name__ == "__main__":
    main()
