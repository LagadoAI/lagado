#!/usr/bin/env python3
"""
eval_g3_retrieval.py — G3 retrieval eval (Phase 0 / Board calibration baseline)

Tests whether the current Jaccard-based retrieval in retrieval.rs surfaces genuinely
relevant memory entries for a set of labelled queries. Build the eval set BEFORE
tuning α/β/γ scoring weights — this is the ground-truth reference that makes any
future tuning falsifiable.

This script runs in two modes:
  --seed    Insert a known set of test memories into memory.db, then exit.
  --eval    Run labelled queries against memory.db and report precision/recall.

Requires: memory.db at LAGADO_DATA_DIR/memory.db (populated by running Lagado at least once,
or by --seed mode below).

Usage:
  python3 evals/eval_g3_retrieval.py --seed   # one-time: populate test memories
  python3 evals/eval_g3_retrieval.py --eval   # run the retrieval quality measurement
  python3 evals/eval_g3_retrieval.py --eval --k 15  # match retrieval.rs default K=15
"""

import argparse
import json
import math
import os
import sqlite3
import sys
import time
import uuid

DATA_DIR = os.environ.get("LAGADO_DATA_DIR", os.path.expanduser("~/.laputa-secure"))
DB_PATH  = os.path.join(DATA_DIR, "memory.db")

# ---------------------------------------------------------------------------
# Seed data: 30 labelled memory entries in three topic clusters.
# Each has an id, text, tier, and a "topic" label for eval purposes.
# We'll store topic in a separate mapping here (not in the DB).
# ---------------------------------------------------------------------------
SEED_ENTRIES = [
    # Cluster A — Firefox / browser
    {"text": "User opened Firefox browser to navigate to a web page",        "tier": "hot",  "topic": "firefox"},
    {"text": "Clicked the address bar in Firefox and typed a URL",           "tier": "hot",  "topic": "firefox"},
    {"text": "Firefox failed to load the page, network error appeared",      "tier": "warm", "topic": "firefox"},
    {"text": "User asked to open a new tab in the browser",                  "tier": "hot",  "topic": "firefox"},
    {"text": "Closed the Firefox window after finishing browsing",           "tier": "warm", "topic": "firefox"},
    {"text": "Browser history was cleared at user's request",                "tier": "cold", "topic": "firefox"},

    # Cluster B — terminal / shell
    {"text": "User ran ls -la in the terminal to list directory contents",   "tier": "hot",  "topic": "terminal"},
    {"text": "Executed git status in the shell to check working tree",       "tier": "hot",  "topic": "terminal"},
    {"text": "Terminal emulator opened with a new bash session",             "tier": "warm", "topic": "terminal"},
    {"text": "User typed cargo build --release in the terminal",             "tier": "hot",  "topic": "terminal"},
    {"text": "Shell command failed with permission denied error",            "tier": "warm", "topic": "terminal"},
    {"text": "User asked to run a python script in the command line",        "tier": "cold", "topic": "terminal"},

    # Cluster C — file management
    {"text": "Moved a file from Downloads to the Documents folder",         "tier": "hot",  "topic": "files"},
    {"text": "User asked to rename a file on the desktop",                  "tier": "hot",  "topic": "files"},
    {"text": "Created a new folder inside the project directory",           "tier": "warm", "topic": "files"},
    {"text": "Deleted old log files from the temporary directory",          "tier": "warm", "topic": "files"},
    {"text": "File explorer opened to browse the home directory",           "tier": "hot",  "topic": "files"},
    {"text": "Searched for a file by name using the find command",          "tier": "cold", "topic": "files"},

    # Noise — unrelated entries that should NOT be retrieved for A/B/C queries
    {"text": "User said hello and asked how the assistant is doing",        "tier": "hot",  "topic": "noise"},
    {"text": "Explained the difference between TCP and UDP protocols",      "tier": "warm", "topic": "noise"},
    {"text": "User asked about the weather and current temperature",        "tier": "cold", "topic": "noise"},
    {"text": "Discussed Python list comprehensions and their syntax",       "tier": "hot",  "topic": "noise"},
    {"text": "User thanked the assistant for helping with a task",          "tier": "warm", "topic": "noise"},
    {"text": "Summarized a document about machine learning architecture",   "tier": "cold", "topic": "noise"},
    {"text": "User asked what time it is and the assistant responded",      "tier": "hot",  "topic": "noise"},
    {"text": "Typed a message in a chat application",                       "tier": "warm", "topic": "noise"},
    {"text": "Scrolled through a settings panel to find a preference",     "tier": "hot",  "topic": "noise"},
    {"text": "User requested a summary of recent actions taken",            "tier": "cold", "topic": "noise"},
    {"text": "Pressed the volume up key to increase system audio",         "tier": "hot",  "topic": "noise"},
    {"text": "User asked the assistant to wait while they thought",        "tier": "warm", "topic": "noise"},
]

# Build id→topic map for eval
SEED_IDS: dict[str, str] = {}  # populated in --seed mode


# ---------------------------------------------------------------------------
# Labelled queries for the eval: (query_text, relevant_topic)
# "relevant" = entries whose topic matches should appear in top-K
# ---------------------------------------------------------------------------
EVAL_QUERIES = [
    ("open Firefox and navigate to google",    "firefox"),
    ("what happened in the browser earlier",   "firefox"),
    ("run a shell command",                    "terminal"),
    ("what was the last terminal command",     "terminal"),
    ("move a file to another folder",          "files"),
    ("find a file in the project",             "files"),
]


# ---------------------------------------------------------------------------
# Jaccard similarity (mirrors retrieval.rs::jaccard_similarity)
# ---------------------------------------------------------------------------
def tokenize(text: str) -> set:
    return set(text.lower().split())

def jaccard(a: str, b: str) -> float:
    sa, sb = tokenize(a), tokenize(b)
    inter = len(sa & sb)
    union = len(sa | sb)
    return inter / union if union else 0.0


def seed_db(db_path: str):
    if not os.path.exists(db_path):
        print(f"ERROR: database not found at {db_path}", file=sys.stderr)
        print("Run Lagado at least once first to create memory.db, then re-run --seed", file=sys.stderr)
        sys.exit(1)

    conn = sqlite3.connect(db_path)
    cur  = conn.cursor()

    inserted = 0
    for entry in SEED_ENTRIES:
        entry_id = str(uuid.uuid4())
        now = int(time.time())
        try:
            cur.execute(
                """INSERT INTO memory_entries
                   (id, text, tier, temperature, created_at, accessed_at, access_count)
                   VALUES (?, ?, ?, ?, ?, ?, ?)""",
                (entry_id, entry["text"], entry["tier"], 1.0, now, now, 1)
            )
            SEED_IDS[entry_id] = entry["topic"]
            inserted += 1
        except sqlite3.IntegrityError:
            pass  # already exists

    conn.commit()
    conn.close()

    # Write the id→topic map next to this script so eval mode can load it
    map_path = os.path.join(os.path.dirname(__file__), "g3_seed_ids.json")
    with open(map_path, "w") as f:
        json.dump(SEED_IDS, f, indent=2)

    print(f"Seeded {inserted} entries into {db_path}")
    print(f"ID→topic map written to {map_path}")
    print("Now run: python3 evals/eval_g3_retrieval.py --eval")


def run_eval(db_path: str, k: int):
    map_path = os.path.join(os.path.dirname(__file__), "g3_seed_ids.json")
    if not os.path.exists(map_path):
        print("ERROR: run --seed first to populate the test entries", file=sys.stderr)
        sys.exit(1)

    with open(map_path) as f:
        id_to_topic = json.load(f)

    conn = sqlite3.connect(db_path)
    cur  = conn.cursor()

    # Load all entries
    cur.execute("SELECT id, text, tier, temperature, accessed_at, access_count FROM memory_entries")
    all_entries = cur.fetchall()
    conn.close()

    print(f"G3 retrieval eval — K={k}, total entries in DB: {len(all_entries)}")
    print()

    total_precision = 0.0
    total_recall    = 0.0

    for query, relevant_topic in EVAL_QUERIES:
        # Score all entries by Jaccard (mirrors retrieval.rs)
        scored = []
        for (eid, text, tier, temp, accessed_at, access_count) in all_entries:
            score = jaccard(query, text)
            scored.append((score, eid, text, tier))

        scored.sort(key=lambda x: x[0], reverse=True)
        top_k = scored[:k]

        # Compute precision and recall against the seed entries of the relevant topic
        seed_relevant = {eid for eid, topic in id_to_topic.items() if topic == relevant_topic}
        retrieved_ids = {eid for (_, eid, _, _) in top_k}

        true_positives = len(retrieved_ids & seed_relevant)
        precision = true_positives / k if k > 0 else 0.0
        recall    = true_positives / len(seed_relevant) if seed_relevant else 0.0
        f1        = (2 * precision * recall / (precision + recall)) if (precision + recall) > 0 else 0.0

        total_precision += precision
        total_recall    += recall

        print(f"Query: '{query}'  (relevant: {relevant_topic})")
        print(f"  Precision@{k}: {precision:.2f}  Recall@{k}: {recall:.2f}  F1: {f1:.2f}")
        print(f"  Top-3 retrieved:")
        for score, eid, text, tier in top_k[:3]:
            topic_label = id_to_topic.get(eid, "other")
            mark = "✓" if eid in seed_relevant else "✗"
            print(f"    {mark} [{tier:4}][{topic_label:8}] score={score:.3f}  '{text[:60]}'")
        print()

    n = len(EVAL_QUERIES)
    mean_p = total_precision / n
    mean_r = total_recall    / n
    mean_f1 = (2 * mean_p * mean_r / (mean_p + mean_r)) if (mean_p + mean_r) > 0 else 0.0
    print(f"=== Mean across {n} queries ===")
    print(f"  Mean Precision@{k}: {mean_p:.2f}")
    print(f"  Mean Recall@{k}:    {mean_r:.2f}")
    print(f"  Mean F1:            {mean_f1:.2f}")
    print()
    print("These numbers are your G3 baseline. Tune α/β/γ ONLY after establishing this.")
    print("A Park-score Board should meaningfully exceed Jaccard on recall.")


def main():
    p = argparse.ArgumentParser()
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--seed", action="store_true", help="Insert test memories into memory.db")
    g.add_argument("--eval", action="store_true", help="Run retrieval quality measurement")
    p.add_argument("--k",    type=int, default=15, help="Retrieval K (default: 15, matches retrieval.rs)")
    p.add_argument("--db",   default=DB_PATH, help=f"Path to memory.db (default: {DB_PATH})")
    args = p.parse_args()

    if args.seed:
        seed_db(args.db)
    else:
        run_eval(args.db, args.k)


if __name__ == "__main__":
    main()
