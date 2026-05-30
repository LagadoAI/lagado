#!/usr/bin/env python3
"""Thalamus: expectation-diff engine for Laputa. Detects command output anomalies."""
import json, os, time, urllib.request

EXPECTATIONS_FILE = "/dev/shm/laputa/expectations.txt"
LAST_CMD_FILE = "/dev/shm/laputa/last_command.txt"
CORTEX_URL = "http://127.0.0.1:8080/v1/chat/completions"
CORTEX_MODEL = "Qwen3-8B-ShiningValiant3.IQ4_XS.gguf"
CHECK_INTERVAL = 2  # seconds

def load_expectations():
    if not os.path.exists(EXPECTATIONS_FILE):
        return {}
    exps = {}
    with open(EXPECTATIONS_FILE) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            parts = line.split("|")
            if len(parts) >= 2:
                exps[parts[0]] = parts[1]
    return exps

def get_last_command_hash():
    if not os.path.exists(LAST_CMD_FILE):
        return None
    with open(LAST_CMD_FILE) as f:
        return f.read().strip()

def post_json(url, payload, timeout=60):
    data = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode())

def notify_cortex(cmd_hash, expected, actual):
    prompt = (
        f"Anomaly detected for command hash {cmd_hash}.\n"
        f"Expected output: {expected[:200]}\n"
        f"Actual output: {actual[:200]}\n"
        "Analyze what went wrong and suggest a fix."
    )
    payload = {
        "model": CORTEX_MODEL,
        "messages": [
            {"role": "system", "content": "You are Laputa's anomaly diagnostician. Diagnose execution anomalies precisely."},
            {"role": "user", "content": prompt}
        ],
        "max_tokens": 256,
        "temperature": 0.4
    }
    try:
        out = post_json(CORTEX_URL, payload)
        response = out["choices"][0]["message"]["content"]
        print(f"CORTEX RESPONSE: {response[:200]}...")
        return response
    except Exception as e:
        print(f"Cortex alert failed: {e}")
        return None

def main():
    os.makedirs("/dev/shm/laputa", exist_ok=True)
    print("Thalamus active — monitoring command expectations...")
    last_seen_hash = None
    while True:
        try:
            current_hash = get_last_command_hash()
            if current_hash and current_hash != last_seen_hash:
                last_seen_hash = current_hash
                exps = load_expectations()
                if current_hash in exps:
                    expected = exps[current_hash]
                    print(f"Command {current_hash[:12]}... executed. Expected: {expected[:60]}...")
                    # In full integration, we'd compare against actual captured output.
                    # For now, we log the expectation for manual verification.
                    # When retina is live, actual output will be read from retina.txt.
                    retina_path = "/dev/shm/laputa/retina.txt"
                    if os.path.exists(retina_path):
                        with open(retina_path) as f:
                            actual = f.read().strip()
                        if actual and actual[:len(expected)] != expected:
                            print(f"MISMATCH DETECTED! Expected: {expected[:60]}... Got: {actual[:60]}...")
                            notify_cortex(current_hash, expected, actual)
                        else:
                            print("Output matches expectation.")
                    else:
                        print("(No retina output yet — expectation recorded)")
        except Exception as e:
            print(f"Thalamus error: {e}")
        time.sleep(CHECK_INTERVAL)

if __name__ == "__main__":
    main()
