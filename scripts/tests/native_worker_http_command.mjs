/** One authenticated HTTP observation/operation for native_worker_execution.py.
 * Fixture credentials arrive on stdin only and never enter argv or artifacts. */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const [runtime,binary,fixture,action]=process.argv.slice(2);
const raw=readFileSync(0);if(raw.length>16384) throw Error('Fixture input too large');
const {binding,owner,grant}=JSON.parse(raw.toString());
const modules=pathToFileURL(`${runtime.replace(/\/$/,'')}/`);
const [{NativeWorker,nativeWorkerRouter},{NativeWorkerHttpTransport},{authMiddleware}]=await Promise.all([
  import(new URL('nativeWorker.js',modules)),import(new URL('nativeWorkerTransport.js',modules)),import(new URL('authMiddleware.js',modules)),
]);
const express=(await import(new URL('../../../node_modules/express/index.js',modules))).default;
process.env.COORDINATOR_TOKEN=owner;
const worker=new NativeWorker({root:join(fixture,'remote'),nodeId:binding.node_id,binary,config:join(fixture,'worker.toml'),
  profiles:async()=>[{name:binding.profile,repo:binding.repo,repo_id:binding.repo_id}],centralToken:()=>owner});
const app=express();app.use(express.json());app.use(authMiddleware);app.use('/api/native-worker',nativeWorkerRouter(worker));
const server=app.listen(0,'127.0.0.1');await new Promise(resolve=>server.once('listening',resolve));
const node={node_id:binding.node_id,advertised_url:`http://127.0.0.1:${server.address().port}`,
  transport_mode:'authenticated_remote',secret_ref:'fixture',profiles:[binding.profile]};
const transport=new NativeWorkerHttpTransport({contextRoot:join(fixture,'runs'),worker:()=>node,secret:()=>owner});
try {
  let result;
  if(action==='stage') {
    result=await transport.stage(binding,join(fixture,'runs',binding.run_id,'context'));
    const deadline=Date.now()+8000;
    while(result.state!=='ready') {
      if(Date.now()>deadline) throw Error('Fixture admission not ready');
      await new Promise(resolve=>setTimeout(resolve,50));result=await transport.observe(binding);
    }
  } else if(action==='start') result=await transport.start(binding,grant);
  else if(action==='observe') result=await transport.observe(binding);
  else if(action==='release-result') result=await transport.releaseResult(binding,{...binding,id:grant.reservation_id,expires_at:grant.expires_at});
  else throw Error('Unknown fixture action');
  console.log(JSON.stringify(result));
} catch {
  process.stderr.write('Native HTTP fixture operation unconfirmed\n');process.exitCode=1;
} finally {
  server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
}
