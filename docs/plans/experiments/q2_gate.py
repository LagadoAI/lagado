import json, urllib.request, collections
N=12
SUBGOAL="Open the Applications menu"
def grammar(labels):
    tgt=" | ".join(f'"{l}"' for l in labels)
    return 'root ::= "complete" | act\n'+'act ::= "act(\\"" target "\\")"\n'+f"target ::= {tgt}\n"
def planner_prompt(labels, fact):
    body="\n".join(f'  "{l}"' for l in labels)
    f = (f'Previous action outcome (deterministic): {fact}\n\n' if fact else "")
    return ("You plan the next step toward a goal on a desktop. Decide if the goal is already "
            "satisfied by the current screen.\n\n"
            f"{f}Goal: {SUBGOAL}\n\n"
            f"Current screen elements:\n{body}\n\n"
            'If the goal is ALREADY satisfied by the current screen, output exactly: complete\n'
            'Otherwise output the next action: act("<element label>")')
def ask(labels, fact):
    g=grammar(labels); p=planner_prompt(labels,fact); c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":p,"grammar":g,"n_predict":20,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","").strip()
            if o.startswith("complete"): c["complete"]+=1
            elif 'act("' in o: c["act:"+o.split('act("')[1].split('"')[0]]+=1
            else: c["?"+o[:10]]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c
def show(c): return ", ".join(f"{k}={v}" for k,v in c.most_common())

# Screens
DESKTOP=["Applications","2026-06-17","Show Desktop","Directory Menu"]              # menu CLOSED (unsatisfied)
MENU_OPEN=["Applications","Directory Menu","Show Desktop",                          # menu OPEN (satisfied);
           "Run Program...","Terminal Emulator","File Manager","Web Browser"]       #   menu items + 'Directory Menu' attractor are salient distractors
FACT='clicked "Applications"; the screen changed and a menu opened with items: Run Program, Terminal Emulator, File Manager, Web Browser.'

print(f"=== Q2 RE-EMIT GATE (N={N}, sub-goal='{SUBGOAL}') ===")
print("    complete=correct-when-satisfied / act(Applications)=RE-EMIT bug / act(other)=DISTRACTOR-GRAB\n")
print("CONTROL — UNSATISFIED screen (menu closed); must NOT complete, should act(Applications):")
print(f"   no-fact : [{show(ask(DESKTOP, None))}]\n")
print("DISCRIMINATOR — SATISFIED screen (menu open, salient distractors present):")
print(f"   no-fact   (expect re-emit/grab): [{show(ask(MENU_OPEN, None))}]")
print(f"   WITH fact (expect complete)    : [{show(ask(MENU_OPEN, FACT))}]")

print("\n=== REFINEMENT: does a LABEL-FREE fact avoid priming + rescue completion? ===")
FACT_LABELFREE='the previous action changed the screen; a menu with 5 items is now open below the top-left button.'
print(f"   label-free fact: [{show(ask(MENU_OPEN, FACT_LABELFREE))}]")
print("   (complete high + no act:Applications => Option 3 salvageable with careful fact wording;")
print("    still ~8/12 or re-emit => Option 2: deterministic goal-state assertion / escalate)")
