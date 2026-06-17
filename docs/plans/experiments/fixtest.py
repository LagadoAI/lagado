import json, urllib.request, collections
SYS = open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL = "Click the Applications menu in the top panel"
N = 12

def run(rows, header_sep="\n"):  # rows = (token,label) in display order
    toks = sorted(set(t for t,_ in rows), key=lambda s:int(s.split("_")[1]))
    targets = " | ".join([f'"{t}"' for t in toks]+['"none"'])
    g = "\n".join(["root ::= click | type | key | wait | done",
        'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"',
        'key ::= "key(key=\\"" freetext "\\")"','wait ::= "wait(ms=" [0-9]+ ")"',
        'done ::= "done(reason=\\"" freetext "\\")"',f"target ::= {targets}",
        'freetext ::= [^"\\\\]*'])+"\n"
    body='On-screen elements (choose one token, or "none" if none fit the goal):'+header_sep
    for t,lab in rows:
        body+=f'  {t}  {chr(34)+lab+chr(34) if lab else "<no label>"}  [a11y]\n'
    prompt=f"{SYS}\n\n{body}\nGoal: {GOAL}\n\nWhat is your next action?"
    picks=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":prompt,"grammar":g,"n_predict":40,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            out=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","")
            tok=out.split('selector="')[1].split('"')[0] if 'selector="' in out else out.strip()[:12]
            picks[tok]+=1
        except Exception as e: picks[f"ERR:{type(e).__name__}"]+=1
    return picks

# G: sacrificial first row, real candidates from el_1; Applications is the 2nd row (el_1).
G=[("el_0","(cursor — not selectable)"),("el_1","Applications"),("el_2",""),("el_3",""),
   ("el_4","2026-06-17"),("el_5","Show Desktop"),("el_6","Directory Menu")]
pg=run(G)
print("G sacrificial first row; Applications=el_1 (2nd row)")
print(f"   Apps(el_1)={pg.get('el_1',0)}/{N}   [{', '.join(f'{k}={v}' for k,v in pg.most_common())}]")

# H: Applications IS the first candidate (el_0) but a blank line separates it from the header.
H=[("el_0","Applications"),("el_1",""),("el_2",""),("el_3","2026-06-17"),
   ("el_4","Show Desktop"),("el_5","Directory Menu")]
ph=run(H, header_sep="\n\n")
print("H blank-line separator; Applications=el_0 (first row)")
print(f"   Apps(el_0)={ph.get('el_0',0)}/{N}   [{', '.join(f'{k}={v}' for k,v in ph.most_common())}]")
