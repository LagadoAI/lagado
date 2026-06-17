import json, urllib.request, collections
SYS=open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL="Click the Applications menu in the top panel"; N=12
def run(rows):
    toks=sorted(set(t for t,_ in rows),key=lambda s:int(s.split("_")[1]))
    targets=" | ".join([f'"{t}"' for t in toks]+['"none"'])
    g="\n".join(["root ::= click | type | key | wait | done",'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"','key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"','done ::= "done(reason=\\"" freetext "\\")"',
        f"target ::= {targets}",'freetext ::= [^"\\\\]*'])+"\n"
    body='On-screen elements (choose one token, or "none" if none fit the goal):\n'
    for t,lab in rows: body+=f'  {t}  {chr(34)+lab+chr(34) if lab else "<no label>"}  [a11y]\n'
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
# I: Applications LAST; the "menu" decoy (Directory Menu) moved to the FRONT.
I=[("el_0","Directory Menu"),("el_1",""),("el_2",""),("el_3","2026-06-17"),("el_4","Show Desktop"),("el_5","Applications")]
p=run(I)
print("I  Applications=el_5 (LAST row); 'Directory Menu' decoy moved to el_0 (first)")
print(f"   Apps(el_5)={p.get('el_5',0)}/{N}   [{', '.join(f'{k}={v}' for k,v in p.most_common())}]")
