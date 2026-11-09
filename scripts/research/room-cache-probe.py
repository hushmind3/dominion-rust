#!/usr/bin/env python3
"""Isolated nine-participant room reproduction, including automatic learning.

Runs real model calls; never submits production messages. Creates a temporary
snapshot containing private data and removes it on normal exit. Fixed loopback
ports 44327/44328 must be free. Usage from failed calls can remain unknown.
"""
import sqlite3,json,pathlib,shutil,os,subprocess,time,urllib.request,urllib.error,threading,http.server,hashlib,uuid,tempfile,argparse
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',required=True);args=parser.parse_args()
ROOT=pathlib.Path(__file__).resolve().parents[2];D=pathlib.Path(tempfile.mkdtemp(prefix='dominion-room-cache-'))
src=sqlite3.connect('file:'+str(ROOT/'runtime/dominion.sqlite')+'?mode=ro',uri=True);db=sqlite3.connect(D/'dominion.sqlite');src.backup(db);src.close();shutil.copyfile(ROOT/'runtime/secret.key',D/'secret.key');os.chmod(D/'secret.key',0o600)
# Keep conversation inputs and evidence; never run copied operational work.
remove=['job','approval','execution','execution_operation','execution_finalization','run_wake','goal','experiment_request','session_cancel','model_job','codex_login']
for k in remove:db.execute('delete from records where kind=?',(k,))
for table in ['audit','event_log','adaptation_finalizations','adaptation_updates']:db.execute('delete from '+table)
routes={}
for rid,raw in db.execute("select id,data from records where kind='provider'").fetchall():
 r=json.loads(raw);base=r.get('baseUrl','');
 if not base:continue
 routes[rid]=base.replace('ACCOUNT_ID',r.get('accountId','')).replace('{accountId}',r.get('accountId','')).rstrip('/');r['baseUrl']='http://127.0.0.1:44328/'+rid
 db.execute("update records set data=? where kind='provider' and id=?",(json.dumps(r),rid))
for rid,raw in db.execute("select id,data from records where kind='room'").fetchall():
 r=json.loads(raw);r['status']='idle';r['activeJobId']=None;r['activeAgentId']=None
 db.execute("update records set data=? where kind='room' and id=?",(json.dumps(r),rid))
db.commit();db.close()
rows=[];prev={};lock=threading.Lock()
def digest(x):return hashlib.sha256(json.dumps(x,sort_keys=True,ensure_ascii=False).encode()).hexdigest()
class Proxy(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  rid,_,path=self.path.lstrip('/').partition('/');body=self.rfile.read(int(self.headers['Content-Length']));b=json.loads(body);session=self.headers.get('session_id',rid);items=b.get('input',b.get('messages',[]));ins=b.get('instructions');old=prev.get(session,{});olditems=old.get('items',[]);prefix=0
  for a,c in zip(items,olditems):
   if a!=c:break
   prefix+=1
  row={'providerId':rid,'model':b.get('model'),'sessionHash':digest(session),'instructionsDigest':digest(ins),'sameInstructions':old.get('instructions')==ins,'itemCount':len(items),'previousItemCount':len(olditems),'prefixItems':prefix,'requestDigest':digest(b),'usage':None}
  prev[session]={'items':items,'instructions':ins}
  headers={k:v for k,v in self.headers.items() if k.lower() not in ['host','content-length','connection']}
  try:
   response=urllib.request.urlopen(urllib.request.Request(routes[rid]+'/'+path,data=body,headers=headers),timeout=180)
  except urllib.error.HTTPError as e:response=e
  row['status']=response.status;self.send_response(response.status);self.send_header('Content-Type',response.headers.get('Content-Type','application/json'));self.send_header('Connection','close');self.end_headers();parts=[]
  try:
   for line in response:
    self.wfile.write(line);self.wfile.flush();parts.append(line)
    if line.startswith(b'data:'):
     try:
      e=json.loads(line[5:]);u=e.get('response',{}).get('usage')
      if u is not None:row['usage']={k:v for k,v in u.items() if k!='attribution'}
     except (ValueError,TypeError):pass
   if row['usage'] is None:
    try:row['usage']=json.loads(b''.join(parts)).get('usage')
    except ValueError:pass
  finally:
   with lock:rows.append(row)
   print(json.dumps({'request':len(rows),'model':row['model'],'status':row['status'],'sameInstructions':row['sameInstructions'],'prefix':prefix,'previous':len(olditems),'usage':row['usage']}),flush=True)
server=http.server.ThreadingHTTPServer(('127.0.0.1',44328),Proxy);threading.Thread(target=server.serve_forever,daemon=True).start()
env=dict(os.environ,DOMINION_DATA=str(D),DOMINION_PORT='44327',DOMINION_WORKSPACE=str(ROOT),DOMINION_BACKUP_INTERVAL_SECS='0',DOMINION_EXPERIMENT_SCHEDULER='0');env.pop('DOMINION_EXPERIMENT_RUNNER',None)
log=open(D/'engine.log','w');proc=subprocess.Popen([str(ROOT/'target/release/dominion-rust'),'--headless'],env=env,stdout=log,stderr=log)
def req(path,body=None):return json.load(urllib.request.urlopen(urllib.request.Request('http://127.0.0.1:44327'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=180))
report={'isolated':True,'autoLearning':True,'toolsEnabled':True,'productionWrites':0,'rows':rows,'rounds':[]}
try:
 for _ in range(60):
  try:req('/api/health');break
  except Exception:time.sleep(.2)
 for turn,prompt in enumerate(['안녕','안녕','안녕'],1):
  start=len(rows);r=req('/api/rooms/nazarick-circle/messages',{'input':prompt,'providerId':'codex-oauth','model':'gpt-6-luna','reasoningEffort':'low','clientMessageId':'isolated-'+uuid.uuid4().hex})
  time.sleep(1)
  for _ in range(240):
   c=sqlite3.connect(D/'dominion.sqlite');jobs=[json.loads(x[0]) for x in c.execute("select data from records where kind='job'")];c.close()
   if not any(j.get('status') in ['queued','running','pending','waiting_approval'] for j in jobs):break
   time.sleep(1)
  c=sqlite3.connect(D/'dominion.sqlite');messages=[json.loads(x[0]) for x in c.execute("select metadata from messages where session_id='room:nazarick-circle' and role='assistant' order by rowid desc limit 9")];c.close()
  summary={'round':turn,'calls':len(rows)-start,'usage':[m.get('usage') for m in messages],'continuity':[m.get('continuity') for m in messages],'jobs':[{'kind':j.get('kind'),'status':j.get('status')} for j in jobs]};report['rounds'].append(summary)
  print('ROUND '+str(turn)+' calls '+str(len(rows)-start),flush=True)
finally:
 proc.terminate()
 try:proc.wait(timeout=10)
 except subprocess.TimeoutExpired:proc.kill();proc.wait()
 server.shutdown();log.close()
 pathlib.Path(args.output).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')

 shutil.rmtree(D)
