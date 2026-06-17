import json, urllib.request, collections
SYS=open("lagado-agent/prompts/system_prompt.txt").read()
N=12
GOAL="Open the Applications menu, then open the Directory Menu"   # two SIMILAR sub-goals
SUB_A="open the Applications menu"; SUB_B="open the Directory Menu"
GRAMMAR=('root ::= "complete" | s\n'
         's ::= "subgoal(\\"" choice "\\")"\n'
         f'choice ::= "{SUB_A}" | "{SUB_B}"\n')
def screen(menu_open):
    base=['Applications','Directory Menu','Show Desktop','2026-06-17']
    if menu_open: base += ['Run Program...','Terminal Emulator','File Manager']
    return "Current screen elements:\n"+"\n".join(f'  "{l}"' for l in base)
def state(kind):
    if kind=="none": return ""
    if kind=="fact": return "Previous action changed the screen; a menu with several items is now open.\n\n"  # LABEL-FREE
    if kind=="progress": return "Progress: completed sub-goal 'open the Applications menu'. Remaining: 'open the Directory Menu'.\n\n"
def prompt(menu_open, kind):
    return (f"{SYS}\n\nYou decompose a multi-step goal into the NEXT sub-goal given the current "
            f"screen and progress.\n\n{state(kind)}Goal: {GOAL}\n\n{screen(menu_open)}\n\n"
            "Emit the NEXT sub-goal toward the goal, or complete if the whole goal is already done.")
def ask(menu_open, kind):
    c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":prompt(menu_open,kind),"grammar":GRAMMAR,"n_predict":24,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","").strip()
            if o.startswith("complete"): c["complete"]+=1
            elif SUB_A in o: c["→step1(Applications)"]+=1
            elif SUB_B in o: c["→step2(Directory)"]+=1
            else: c["?"+o[:12]]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c
def show(c): return ", ".join(f"{k}={v}" for k,v in c.most_common())
print(f"=== DECOMPOSITION PROBE (N={N}) goal='{GOAL}' — two similar sub-goals ===")
print("    discriminator: at the mid-trajectory screen (Apps menu open), does the planner ADVANCE to step2?\n")
print(f"A  desktop (start),    state=none    -> expect step1: [{show(ask(False,'none'))}]")
print(f"B  Apps-menu-OPEN, LABEL-FREE fact   -> expect step2: [{show(ask(True,'fact'))}]   <- KILLER (can label-free fact disambiguate which menu opened?)")
print(f"C  Apps-menu-OPEN, PROGRESS state    -> expect step2: [{show(ask(True,'progress'))}]")
print(f"D  Apps-menu-OPEN, state=none        -> baseline:    [{show(ask(True,'none'))}]")
