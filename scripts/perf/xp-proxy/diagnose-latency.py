import json,sys,subprocess,time,os
from pathlib import Path
repo=Path(os.environ.get('PROBE_TOOLS','/Users/ivan/.codex/worktrees/xp-proxy-performance/xp'))
sys.path.insert(0,str(repo/'scripts/perf/xp-proxy'))
from benchmark import Processes,configs,port,ready
root=Path(os.environ.get('PROBE_OUTPUT',str(Path(__file__).parent)));root.mkdir(parents=True,exist_ok=True)
harness=Path(os.environ.get('PROBE_HARNESS',str(repo/'target/release/examples/perf_harness')))
xray=Path(os.environ.get('PROBE_XRAY','/tmp/xp-proxy-performance-489/xray-macos/bin/xray'))
binary=Path(sys.argv[1]); label=sys.argv[2]
results=[]
for direction in ('inbound','outbound'):
 d=root/f'{label}-{direction}';d.mkdir(exist_ok=True)
 group=Processes(d)
 try:
  origin,remote,local,fallback=(port() for _ in range(4))
  p=group.start('origin',[harness,'--mode','origin','--addr',f'127.0.0.1:{origin}']);ready(origin,p)
  p=group.start('fallback',['python3',repo/'scripts/perf/xp-proxy/tls_origin.py','--port',fallback,'--directory',d]);ready(fallback,p)
  server,client,snapshot,outbound=configs(d,origin,remote,local,fallback)
  if direction=='inbound':
   subject=group.start('xp-proxy',[binary,'--control-socket',d/'control.sock','--state-file',d/'state.json','--snapshot',snapshot]);ready(remote,subject)
   p=group.start('xray-client',[xray,'run','-config',client]);ready(local,p)
  else:
   p=group.start('xray-server',[xray,'run','-config',server]);ready(remote,p)
   subject=group.start('xp-proxy',[binary,'--outbound-config',outbound]);ready(local,subject)
  args=[str(harness),'--mode','load','--addr',f'127.0.0.1:{local}','--concurrency','1','--workload','latency','--duration-ms','10000']
  warm=args+['--requests-per-client','1']
  subprocess.check_output(warm,timeout=12)
  args[args.index('--duration-ms')+1]='2000'
  for sweep in range(2):
   r=json.loads(subprocess.check_output(args,timeout=5))
   entry=dict(label=label,direction=direction,sweep=sweep,**r);results.append(entry)
   print(json.dumps({k:entry[k] for k in ('label','direction','sweep','errors','samples','p50_ms','p95_ms')}),flush=True)
  if direction=='outbound' and Path('/proc').exists():
   def fds():
    paths=list(Path(f'/proc/{subject.pid}/fd').iterdir())
    return {p.name:os.readlink(p) for p in paths if p.exists()}
   before=fds()
   bulk=args.copy();bulk[bulk.index('--concurrency')+1]='8'
   r=json.loads(subprocess.check_output(bulk,timeout=5))
   observations=[]
   for delay in (0,0.2,1,3):
    time.sleep(delay);observations.append(dict(delay=delay,fds=fds()))
   entry=dict(label=label,direction=direction,resource_probe=True,before=before,after=observations,result=r)
   results.append(entry);print(json.dumps(entry),flush=True)
 finally:group.stop()
(root/f'{label}.json').write_text(json.dumps(results,indent=2))
# This temporary causal probe records failures rather than changing acceptance.

