import assert from 'node:assert/strict';
import EmbeddedPostgres from 'embedded-postgres';
import { chromium } from '@playwright/test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, readdir, mkdir, access, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { createServer } from 'node:net';
import { setupTools, mailpitPath, toolsDir } from '../scripts/setup-tools.mjs';

const cargo=process.env.CARGO||'cargo';
const port=(name,fallback)=>Number(process.env[`FRIZZI_TEST_${name}_PORT`]||fallback);
const pgPort=port('PG',55433), smtpPort=port('SMTP',11025), mailUiPort=port('MAIL_UI',18025), apiPort=port('API',13001), webPort=port('WEB',15173);
// Multiplies every polling budget (and the default watchdog) for slow machines.
const scale=Number(process.env.FRIZZI_TEST_TIMEOUT_SCALE||1);if(!(scale>0))throw new Error('FRIZZI_TEST_TIMEOUT_SCALE must be a positive number');
const directory=await mkdtemp(join(tmpdir(),'frizzi-integration-'));
const children=[];let browser;let database;let client;let stalledSmtp;
const stalledSockets=new Set();const contexts=[];
const web=`http://127.0.0.1:${webPort}`, api=`http://127.0.0.1:${apiPort}`, mail=`http://127.0.0.1:${mailUiPort}`;
const cronSecret=randomUUID()+randomUUID();
const logDir=join(directory,'logs');
const env={...process.env,DATABASE_URL:`postgres://frizzi:frizzi@127.0.0.1:${pgPort}/frizzi_test`,APP_URL:web,MAIL_FROM:'no-reply@eventi.relistim.it',SMTP_HOST:'127.0.0.1',SMTP_PORT:String(smtpPort),RESEND_API_KEY:'',GEOAPIFY_API_KEY:'',GEMINI_API_KEY:'',FRIZZI_TYPST:process.env.FRIZZI_TYPST||'',API_BIND:`127.0.0.1:${apiPort}`,API_PROXY_TARGET:api,CRON_SECRET:cronSecret,FRIZZI_LOG_DIR:logDir,FRIZZI_PROBE_PRIMARY:'',FRIZZI_PROBE_LEGACY:''};
const mailpitEnv={env:{...env,MP_SMTP_BIND_ADDR:`127.0.0.1:${smtpPort}`,MP_UI_BIND_ADDR:`127.0.0.1:${mailUiPort}`}};
const expectPdf=!!env.FRIZZI_TYPST&&!process.env.FRIZZI_TEST_PDF_FALLBACK;
function start(command,args,extra={}){
 const child=spawn(command,args,{env,...extra});children.push(child);
 let log='';child.output=()=>log;child.stdout?.on('data',data=>log+=data);child.stderr?.on('data',data=>log+=data);
 child.on('exit',code=>{if(code && code!==null && !cleaning)console.error(log.slice(-5000));});return child;
}
async function run(command,args){
 const child=spawn(command,args,{env,stdio:'inherit'});child.output=()=>'';children.push(child);
 const [code,signal]=await once(child,'exit');if(code!==0)throw new Error(`${command} ${args.join(' ')} failed: ${signal||code}`);
}
function settle(promise,ms){let timer;return Promise.race([Promise.resolve(promise).catch(()=>{}),new Promise(resolve=>timer=setTimeout(resolve,ms))]).finally(()=>clearTimeout(timer));}
async function stop(child){
 if(child.exitCode!==null||child.signalCode!==null||!child.pid)return;
 const exited=once(child,'exit');child.kill('SIGTERM');
 const timer=setTimeout(()=>child.kill('SIGKILL'),3000);await settle(exited,6000);clearTimeout(timer);
}
let cleaning;
function cleanup(){return cleaning??=(async()=>{
 clearTimeout(watchdog);
 for(const socket of stalledSockets)socket.destroy();
 stalledSmtp?.close();
 await settle(browser?.close(),5000);
 await Promise.all(children.map(stop));
 await settle(client?.end(),2000);
 if(database)await settle(database.stop(),5000);
 await rm(directory,{recursive:true,force:true}).catch(error=>console.error(`Could not remove ${directory}: ${error.message}`));
})();}
let saving;
function saveArtifacts(){return saving??=(async()=>{
 if(!contexts.length)return;
 await mkdir('test-results',{recursive:true});
 for(const [i,context] of contexts.entries()){
  for(const [j,page] of context.pages().entries())await page.screenshot({path:`test-results/failure-${i}-${j}.png`,fullPage:true,timeout:5000}).catch(error=>console.error(`Failure screenshot not saved: ${error.message}`));
  await settle(context.tracing.stop({path:`test-results/failure-trace-${i}.zip`}),10000);
 }
 console.error('Failure screenshots and Playwright traces saved in test-results/ (open with: npx playwright show-trace <zip>).');
})().catch(error=>console.error(`Failure artifacts not saved: ${error.message}`));}
async function fail(error){if(interrupted)return;console.error(error);for(const child of children)console.error(child.output().slice(-2500));process.exitCode=1;await saveArtifacts();}
const watchdogMs=Number(process.env.FRIZZI_TEST_WATCHDOG_MS||15*60*1000*scale);
const watchdog=setTimeout(async()=>{await fail(new Error(`Watchdog: the suite did not finish within ${watchdogMs} ms`));await cleanup();process.removeAllListeners('exit');process.exit(1);},watchdogMs);
// embedded-postgres (through exit-hook) exits on these signals before the other services are stopped,
// so the runner owns them: clean up everything, then re-raise to exit with the conventional status.
// npm forwards the terminal's Ctrl-C, so a second signal arrives at once: only a later one forces an exit.
let interrupted=0;
for(const signal of ['SIGINT','SIGTERM']){process.removeAllListeners(signal);process.on(signal,async()=>{
 if(interrupted){if(Date.now()-interrupted>3000){console.error('Forced exit: cleanup skipped.');process.exit(1);}return;}
 interrupted=Date.now();console.error(`\nReceived ${signal}, cleaning up...`);
 await cleanup();process.removeAllListeners(signal);process.kill(process.pid,signal);
});}
async function newPage(options){
 const context=await browser.newContext(options);contexts.push(context);context.setDefaultTimeout(30000*scale);
 await context.tracing.start({screenshots:true,snapshots:true});
 const page=await context.newPage();page.once('close',()=>{contexts.splice(contexts.indexOf(context),1);context.close().catch(()=>{});});return page;
}
async function waitFor(url){
 for(let i=0;i<150*scale;i++){try{const r=await fetch(url);if(r.ok)return;}catch{}await new Promise(r=>setTimeout(r,100));}
 throw new Error(`Service failed to start: ${url}`);
}
async function call(path,method='GET',body,expected=200){
 const response=await fetch(web+path,{method,headers:body?{'Content-Type':'application/json'}:{},body:body?JSON.stringify(body):undefined});
 const text=await response.text();assert.equal(response.status,expected,`${method} ${path} returned ${response.status}: ${text}`);
 return JSON.parse(text);
}
async function messagesFor(address){
 // Search server-side and page through results, so a busy inbox never hides older messages; newest first.
 const found=[],limit=250,query=encodeURIComponent(`to:"${address}"`);
 for(let start=0;;start+=limit){
  const data=await(await fetch(`${mail}/api/v1/search?query=${query}&start=${start}&limit=${limit}`)).json();
  found.push(...data.messages);if(data.messages.length<limit)break;
 }
 return found.filter(m=>m.To.some(to=>to.Address===address));
}
async function until(find,description){
 for(let i=0;i<100*scale;i++){
  const result=await find();if(result)return result;
  await new Promise(resolve=>setTimeout(resolve,50));
 }
 throw new Error(description);
}
async function messageFor(address,subject=''){
 const entry=await until(async()=>(await messagesFor(address)).find(m=>m.Subject.includes(subject)),`No mail for ${address} matching ${subject}`);
 return (await fetch(`${mail}/api/v1/message/${entry.ID}`)).json();
}
async function queueDrained(){
 await until(async()=>(await client.query('SELECT count(*) FROM email_outbox WHERE sent_at IS NULL AND failed_at IS NULL')).rows[0].count==='0','Email queue did not drain');
}
const tokenPattern=new RegExp(web.replace(/[.*+?^$()|[\]\\{}]/g,'\\$&')+'/[\\w-]{21}(?:/admin)?/([\\w-]{21})');
function tokenFrom(text){return text.match(tokenPattern)[1];}
// Date-relative retention checks must not straddle midnight in the zone the server evaluates them in.
async function clearOfMidnight(zone,margin=60*scale){
 const seconds=Number((await client.query("SELECT extract(epoch FROM date_trunc('day',now() AT TIME ZONE $1)+interval '1 day'-(now() AT TIME ZONE $1)) AS s",[zone])).rows[0].s);
 if(seconds<margin){console.log(`Waiting ${Math.ceil(seconds)}s for midnight in ${zone} to pass.`);await new Promise(r=>setTimeout(r,(seconds+1)*1000));}
}
const initial={title:'Cena <b>insieme</b>',description:'Una sera insieme',location:'Roma',organizer_name:'Ada',organizer_email:'ada@example.test',locale:'it',allow_multiple_choices:true,timezone:'Europe/Rome',slots:[{date:'2030-02-03',start:'20:00',end:'22:00'},{date:'2030-02-04',start:'20:00',end:null}]};
let checks=0;
function check(condition,message){assert.ok(condition,message);checks++;}
try{
 await setupTools();
 if(!process.env.SKIP_BUILD)await run(cargo,['build','--bins']);
 database=new EmbeddedPostgres({databaseDir:join(directory,'pg'),user:'frizzi',password:'frizzi',port:pgPort,persistent:false,onLog:()=>{},onError:()=>{}});
 await database.initialise();await database.start();await database.createDatabase('frizzi_test');
 client=database.getPgClient('frizzi_test');await client.connect();
 check((await client.query('SHOW server_version')).rows[0].server_version.startsWith('16.'),'PostgreSQL 16 required');
 await run(join(process.cwd(),'target/debug/migrate'),[]);
 const mailChild=start(mailpitPath,[],mailpitEnv);
 await waitFor(mail+'/api/v1/messages');
 start(join(process.cwd(),'target/debug/local'),[]);
 start(process.execPath,['node_modules/vite/bin/vite.js','dev','--host','127.0.0.1','--port',String(webPort),'--strictPort']);
 await waitFor(api+'/api/health');await waitFor(web);
 check((await fetch(api+'/api/ready')).ok,'Readiness checks the migrated database');
 check((await fetch(api+'/api/telemetry',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify('share_copy')})).status===204,'Allowlisted telemetry is accepted');
 check((await fetch(api+'/api/telemetry',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify('x'.repeat(300))})).status===413,'Telemetry has a dedicated body limit');
 console.log('PostgreSQL 16, Mailpit, Rust API and SvelteKit ready.');

 // Validate all locale dictionaries, including email templates.
 const locales=['it','en','es','zh','hi','ar','fr','bn','pt','ru','de'];
 const italian=JSON.parse(await readFile('locales/it.json','utf8'));
 for(const locale of locales){
  const dictionary=JSON.parse(await readFile(`locales/${locale}.json`,'utf8'));
  assert.deepEqual(Object.keys(dictionary.ui).sort(),Object.keys(italian.ui).sort());
  assert.deepEqual(Object.keys(dictionary.mail).sort(),Object.keys(italian.mail).sort());
  check(Object.values(dictionary.ui).every(value=>typeof value==='string'&&value.length),'Missing UI translation');
 }
 check((await fetch(web+'/not-an-event')).status===404&&(await fetch(web+'/favicon.svg')).headers.get('content-type').includes('svg'),'Malformed links are real 404s and the favicon is served');
 await call('/api/events','POST',{...initial,slots:[initial.slots[0],initial.slots[0]]},400);
 await call('/api/events','POST',{...initial,timezone:'invalid/timezone'},400);
 check((await call('/api/places/suggest','POST',{text:'Via Roma Torino'})).enabled===false,'Location lookup is optional');
 await call('/api/places/suggest','POST',{text:'https://example.test'},400);
 await call('/api/events','POST',{...initial,location_geo:{latitude:95,longitude:12,kind:'address'}},400);
 const earlyDeadline=await call('/api/events','POST',{...initial,voting_deadline:'2000-01-01'},400);
 check(earlyDeadline.error==='invalid_deadline','A new deadline cannot be in the past');
 await call('/api/events','POST',{...initial,slots:[{...initial.slots[0],end:'19:00'}]},400);
 for(const date of ['2000-01-01','2099-01-01'])check((await call('/api/events','POST',{...initial,slots:[{...initial.slots[0],date}]},400)).error==='invalid_dates',`Proposed date ${date} is out of range`);
 check((await call('/api/events','POST',{...initial,voting_deadline:'2030-02-05'},400)).error==='invalid_deadline','Voting cannot close after the last proposed date');
 const blank=await call('/api/events','POST',{...initial,description:' \n\t ',organizer_email:'blank@example.test'});
 check((await call(`/api/events/${blank.public_token}`)).description===null,'A blank description is stored as none');
 for(const locale of locales.filter(code=>code!=='it')){
  const address=`locale-${locale}@example.test`;
  const result=await call('/api/events','POST',{...initial,organizer_email:address,locale});
  const dictionary=JSON.parse(await readFile(`locales/${locale}.json`,'utf8'));
  const email=await messageFor(address);
  check(email.Subject.startsWith('🔑 '+dictionary.mail.adminSubject),`Emoji and localized email subject for ${locale}`);
  check(email.Text.includes(dictionary.mail.adminBody),`UTF-8 email body for ${locale}`);
 }
 const created=await call('/api/events','POST',{...initial,voting_deadline:'2030-02-01'});
 const publicToken=created.public_token;
 check(/^[\w-]{21}$/.test(publicToken),'Public token format');
 check(!JSON.stringify(created).includes('admin'),'Secret absent from creation response');
 const adminMail=await messageFor(initial.organizer_email);const adminToken=tokenFrom(adminMail.Text);
 check(adminMail.Subject.includes('Il tuo link organizzatore'),'Italian organizer mail');
 let event=await call(`/api/events/${publicToken}`);
 check(event.slots.length===2 && event.participants.length===0,'Event persisted');
 check(event.voting_deadline==='2030-02-01' && !event.voting_ended,'Future voting deadline saved');
 const tiePath=`/api/events/${publicToken}/admin/${adminToken}/tiebreak`;
 await call(tiePath,'PUT',{slot_id:event.slots[0].id},409);
 check((await fetch(web+`/api/events/${publicToken}/calendar.ics`)).status===404,'Calendar only for confirmed events');
 const single=await call('/api/events','POST',{...initial,title:'Singola scelta',organizer_email:'single@example.test',allow_multiple_choices:false,locale:'en'});
 const singleEvent=await call(`/api/events/${single.public_token}`);
 const singleAdmin=tokenFrom((await messageFor('single@example.test')).Text);
 await call(`/api/events/${publicToken}/admin/${'A'.repeat(21)}`,'GET',undefined,404);
 await call(`/api/events/${single.public_token}/admin/${adminToken}`,'GET',undefined,404);
 const v={name:'Lucía',email:'lucia@example.test',locale:'es',slots:event.slots.map(s=>s.id)};
 await call(`/api/events/${publicToken}/votes`,'POST',{...v,slots:[singleEvent.slots[0].id]},400);
 await call(`/api/events/${publicToken}/votes`,'POST',{...v,slots:[event.slots[0].id,event.slots[0].id]},400);
 await call(`/api/events/${single.public_token}/votes`,'POST',{...v,slots:singleEvent.slots.map(s=>s.id)},400);
 await call(`/api/events/${publicToken}/votes`,'POST',v);
 const voteMail=await messageFor(v.email);const editToken=tokenFrom(voteMail.Text);
 check(voteMail.Subject.includes('Tu respuesta'),'Spanish participant mail');
 await call(`/api/events/${publicToken}/votes`,'POST',{...v,email:' LUCIA@example.test '},409);
 await call(`/api/events/${publicToken}/votes`,'POST',{name:'Nessuna data',email:'none@example.test',locale:'de',slots:[]});
 event=await call(`/api/events/${publicToken}`);
 check(event.participants.length===2,'Unique email and empty availability');
 const publicJson=JSON.stringify(event);
 for(const privateValue of [initial.organizer_email,v.email,adminToken,editToken])check(!publicJson.includes(privateValue),'Private data leaked');
 check(!/email|token/i.test(Object.keys(event).join(' ')),'Private API field leaked');
 const response=await fetch(web+`/api/events/${publicToken}`);check(response.headers.get('Cache-Control')==='no-store','API cache prevention');
 const person=await call(`/api/events/${publicToken}/participant/${editToken}`);check(person.email===v.email,'Private edit access');
 await call(`/api/events/${publicToken}/admin/${editToken}/tiebreak`,'PUT',{slot_id:event.slots[0].id},404);
 await call(tiePath,'PUT',{slot_id:singleEvent.slots[0].id},409);
 await call(tiePath,'PUT',{slot_id:event.slots[0].id});
 const broken=await call(`/api/events/${publicToken}`);
 check(broken.tiebreak_slot_id===event.slots[0].id&&broken.status==='OPEN','Organizer records the deciding vote without confirming the event');
 assert.deepEqual(broken.participants,event.participants);
 await call(tiePath,'PUT',{slot_id:null});
 check((await call(`/api/events/${publicToken}`)).tiebreak_slot_id===null,'Organizer can withdraw the deciding vote');
 await call(tiePath,'PUT',{slot_id:event.slots[0].id});
 await call(`/api/events/${publicToken}/participant/${editToken}`,'PUT',{...v,slots:[event.slots[1].id]});
 check((await call(`/api/events/${publicToken}`)).tiebreak_slot_id===null,'A new ranking clears an invalid deciding vote');
 await call(`/api/events/${single.public_token}/participant/${editToken}`,'GET',undefined,404);
 await call(`/api/events/${publicToken}/participant/${'Z'.repeat(21)}`,'PUT',v,404);
 await call(`/api/events/${publicToken}/participant/${editToken}`,'PUT',{...v,name:'Lucía actualizada'});
 const eventPath=`/api/events/${publicToken}`;
 const details={...event,locale:'it',slots:event.slots.map(s=>({...s,start:s.start,end:s.end}))};
 const editPath=`${eventPath}/admin/${adminToken}/details`;
 const {voting_deadline: originalDeadline,...detailsWithoutDeadline}=details;
 await call(editPath,'PUT',detailsWithoutDeadline);
 check((await call(eventPath)).voting_deadline===originalDeadline,'Legacy update preserves deadline');
 await client.query("UPDATE events SET voting_deadline='2000-01-01' WHERE public_token=$1",[publicToken]);
 check((await call(eventPath)).voting_ended,'Expired deadline visible in API');
 check((await call(eventPath+'/votes','POST',{...v,email:'late@example.test'},409)).error==='deadline_passed','Deadline blocks new votes');
 check((await call(eventPath+`/participant/${editToken}`,'PUT',v,409)).error==='deadline_passed','Deadline blocks vote changes');
 await call(editPath,'PUT',{...details,voting_deadline:null});
 check((await call(eventPath)).voting_deadline===null && !(await call(eventPath)).voting_ended,'Removing deadline reopens voting');
 await call(eventPath+`/participant/${editToken}`,'PUT',{...v,name:'Lucía actualizada'});
 await queueDrained();
 const generic=await call(eventPath+'/recover','POST',{email:'unknown@example.test'});
 check((await messagesFor('unknown@example.test')).length===0,'Recovery does not send to unknown addresses');
 for(const [address,privateToken] of [[v.email,editToken],[initial.organizer_email,adminToken]]){
  const before=(await messagesFor(address)).length;
  for(let i=0;i<4;i++){
   assert.deepEqual(await call(eventPath+'/recover','POST',{email:address}),generic);
  }
  for(let i=0;i<80*scale&&(await messagesFor(address)).length<before+3;i++)await new Promise(r=>setTimeout(r,50));
  check((await messagesFor(address)).length===before+3,'Recovery mails are capped per recipient');
  check(tokenFrom((await messageFor(address)).Text)===privateToken,'Recovery uses the existing private link');
 }
 await call(`/api/events/${publicToken}/admin/${adminToken}/details`,'PUT',{...details,allow_multiple_choices:false},409);
 await call(`/api/events/${publicToken}/admin/${adminToken}/details`,'PUT',{...details,location:'Milano'});
 await queueDrained();
 const diffMail=await messageFor(v.email,'El evento ha cambiado');
 check(diffMail.Text.includes('Roma')&&diffMail.Text.includes('Milano')&&diffMail.Text.includes('Qué ha cambiado'),`Localized change diff: ${diffMail.Subject} ${diffMail.Text.replace(/http:\/\/[^\s]+/g,'[private-link]')}`);
 check(!diffMail.Text.includes(adminToken),'Organizer token leaked to participant email');
 const statusPath=`/api/events/${publicToken}/admin/${adminToken}`;
 await call(statusPath,'PUT',{status:'CONFIRMED',confirmed_slot_id:singleEvent.slots[0].id,locale:'it'},400);
 await call(statusPath,'PUT',{status:'CONFIRMED',confirmed_slot_id:event.slots[0].id,locale:'it'});
 check((await call(`/api/events/${publicToken}`)).status==='CONFIRMED','Event confirmation');
 const calendarResponse=await fetch(web+eventPath+'/calendar.ics');
 const ical=await calendarResponse.text();
 check(calendarResponse.ok && calendarResponse.headers.get('content-type').startsWith('text/calendar'),'Calendar download');
 check(ical.includes('DTSTART:20300203T190000Z') && ical.includes('DTEND:20300203T210000Z'),'Calendar preserves the event time zone');
 check(!ical.includes(adminToken) && !ical.includes(editToken),'Calendar contains no private links');
 check(ical.includes('\r\nMETHOD:PUBLISH\r\n')&&ical.includes(`\r\nUID:${publicToken}@frizzi\r\n`)&&/\r\nSEQUENCE:\d+\r\n/.test(ical),'Calendar is a publishable revision with a stable UID');
 const confirmation=await messageFor(v.email,'🎉 ¡Ya tenemos fecha!');
 const confirmedMail=confirmation.Text;
 check(confirmedMail.includes(eventPath+'/calendar.ics'),'Calendar URL in confirmation email');
 check(confirmedMail.includes('03/02/2030'),'Confirmation email uses the recipient’s numeric date format');
 check(confirmedMail.includes('¡Ya tenemos fecha!')&&confirmedMail.includes('Cómo llegar'),'Localized confirmation fallback with directions');
 check(confirmedMail.includes('https://www.google.com/maps/dir/?api=1&destination=Milano'),'Confirmation directions use the current location');
 check(tokenFrom(confirmedMail)===editToken,'Confirmation preserves the participant’s private link');
 check(confirmation.HTML.includes('¡Ya tenemos fecha! 🎉')&&confirmation.HTML.includes('¡Qué ganas de vernos!'),'Warm confirmation in HTML');
 check(confirmation.HTML.includes('Cena &lt;b&gt;insieme&lt;/b&gt;')&&!confirmation.HTML.includes('Cena <b>insieme</b>'),'HTML escapes event titles');
 check(confirmation.HTML.includes(`href="${web}${eventPath}/calendar.ics"`)&&confirmation.HTML.includes('api=1&amp;destination=Milano'),'Calendar and directions are clickable in HTML');
 check(confirmation.HTML.includes(`href="${web}/${publicToken}/${editToken}"`)&&!confirmation.HTML.includes(adminToken),'HTML keeps the recipient’s private link');
 if(expectPdf){
  const attachment=confirmation.Attachments.find(part=>part.FileName==='invito-frizzi.pdf');
  check(attachment?.ContentType==='application/pdf'&&attachment.Size<=512*1024,'Compact PDF is attached to confirmation');
  const response=await fetch(`${mail}/api/v1/message/${confirmation.ID}/part/${attachment.PartID}`);
  const pdf=Buffer.from(await response.arrayBuffer());
  check(response.ok&&pdf.subarray(0,5).toString()==='%PDF-','Attachment downloads as a valid PDF stream');
 }else{
  check((confirmation.Attachments||[]).length===0,'Confirmation still arrives when PDF is unavailable');
 }
 await call(`/api/events/${publicToken}/participant/${editToken}`,'PUT',v,409);
 const countBefore=(await messagesFor(v.email)).length;
 await call(statusPath,'PUT',{status:'CONFIRMED',confirmed_slot_id:event.slots[0].id,locale:'it'});
 check((await messagesFor(v.email)).length===countBefore,'No duplicate unchanged-status email');
 await call(`/api/events/${publicToken}/admin/${adminToken}/details`,'PUT',{...details,location:'Milano',slots:[{...details.slots[0],start:'19:00:00'},details.slots[1]]});
 event=await call(`/api/events/${publicToken}`);
 check(event.status==='OPEN'&&event.confirmed_slot_id===null,'Date change reopens poll');
 check(event.participants[0].slots.length===1&&event.participants[0].slots[0]===details.slots[1].id,'Unaffected votes preserved');
 await call(statusPath,'PUT',{status:'CANCELLED',confirmed_slot_id:null,locale:'it'});
 check((await messageFor(v.email,'Evento cancelado')).Subject.includes('📌'),'Cancellation notification');
 await call(statusPath,'PUT',{status:'OPEN',confirmed_slot_id:null,locale:'it'});
 check((await messageFor(v.email,'Encuesta reabierta')).Subject.includes('🗳️'),'Reopening notification');
 await queueDrained();
 const pending=(await client.query('SELECT count(*) FROM email_outbox WHERE sent_at IS NULL AND failed_at IS NULL')).rows[0].count;
 check(pending==='0','Email queue drained');
 check((await client.query("SELECT count(*) FROM email_outbox WHERE sent_at IS NOT NULL AND (body<>'' OR html IS NOT NULL OR recipient<>'')")).rows[0].count==='0','Delivered secrets removed from both formats in the outbox');
 const unauthorized=await fetch(api+'/api/cron/email');check(unauthorized.status===401,'Cron requires secret');
 const wrongSecret=cronSecret.slice(0,-1)+(cronSecret.endsWith('0')?'1':'0');
 check((await fetch(api+'/api/cron/email',{headers:{Authorization:`Bearer ${wrongSecret}`}})).status===401,'Cron rejects a wrong secret of the same length');

 // Delivery failure must preserve the event and retry without exposing its private token.
 mailChild.kill('SIGTERM');await new Promise(resolve=>mailChild.once('exit',resolve));
 stalledSmtp=createServer(socket=>{
  stalledSockets.add(socket);
  socket.on('error',()=>{});
  socket.on('close',()=>stalledSockets.delete(socket));
 });
 await new Promise(resolve=>stalledSmtp.listen(smtpPort,'127.0.0.1',resolve));
 const updates=statusPath+'/messages';
 const broadcast={request_id:randomUUID(),message:'<b>Ci vediamo!</b>\n<i>A presto</i> <img src=x onerror=alert(1)>'};
 await call(`${eventPath}/admin/${editToken}/messages`,'POST',broadcast,404);
 await call(updates,'POST',{request_id:randomUUID(),message:'<strong> </strong>'},400);
 const started=performance.now();
 const deliveries=await Promise.all([call(updates,'POST',broadcast),call(updates,'POST',broadcast)]);
 assert.deepEqual(deliveries,[{queued:2},{queued:2}]);
 check(performance.now()-started<2500,'The page gets a queue receipt while SMTP is stalled');
 check((await client.query("SELECT count(*) FROM email_outbox WHERE sent_at IS NULL AND subject LIKE '💌%'")).rows[0].count==='2','Concurrent retries enqueue once per participant');
 await until(()=>stalledSockets.size,'SMTP worker did not attempt delivery');
 const stopped=new Promise(resolve=>stalledSmtp.close(resolve));
 for(const socket of stalledSockets) socket.destroy();
 await stopped;
 stalledSmtp=null;
 const deferred=await call('/api/events','POST',{...initial,organizer_email:'retry@example.test'});
 check((await call(`/api/events/${deferred.public_token}`)).title===initial.title,'Event survives SMTP outage');
 check((await client.query("SELECT count(*) FROM email_outbox WHERE recipient='retry@example.test' AND sent_at IS NULL")).rows[0].count==='1','Failed delivery remains queued');
 await until(async()=>(await client.query("SELECT count(*) FROM email_outbox WHERE sent_at IS NULL AND (attempts=0 OR locked_until IS NOT NULL)")).rows[0].count==='0','Provider failure is recorded');
 start(mailpitPath,[],mailpitEnv);
 await waitFor(mail+'/api/v1/messages');
 await client.query("INSERT INTO email_outbox (recipient,subject,body) VALUES ('legacy@example.test','Pending before upgrade','Plain text')");
 await client.query("INSERT INTO email_outbox (recipient,subject,body,html) VALUES ('no-at-sign.example.test','Rejected','Private text','Private HTML')");
 await client.query('UPDATE email_outbox SET available_at=now() WHERE sent_at IS NULL');
 const retried=await fetch(api+'/api/cron/email',{headers:{Authorization:`Bearer ${cronSecret}`}});check(retried.ok,'Retry worker');
 check((await messageFor('retry@example.test')).Text.includes('/admin/'),'Private link delivered after retry');
 check((await messageFor('legacy@example.test')).Text.trim()==='Plain text','Queued plain text from before the upgrade still delivers');
 await until(async()=>(await client.query("SELECT count(*) FROM email_outbox WHERE subject='' AND failed_at IS NOT NULL AND sent_at IS NULL AND attempts=1 AND recipient='' AND body='' AND html IS NULL")).rows[0].count==='1','A rejected message fails at once and is scrubbed');
 check(true,'Permanent delivery failures are terminal and drop private content');
 const updateMail=await messageFor(v.email,'💌 ¡Una novedad para ti!');
 check(updateMail.HTML.includes('<b>Ci vediamo!</b>')&&updateMail.HTML.includes('<i>A presto</i>'),'Participant message preserves allowed formatting');
 check(updateMail.HTML.includes('&lt;img')&&!updateMail.HTML.includes('<img'),'Participant message escapes unsupported HTML');
 check(updateMail.Text.includes('Ci vediamo!')&&!updateMail.Text.includes('<b>Ci vediamo!'),'Participant message includes a plain text alternative');
 check(updateMail.To.length===1&&updateMail.HTML.includes(`href="${web}/${publicToken}"`)&&!updateMail.HTML.includes(adminToken),'Only the recipient and shared event link are included');
 check((await messageFor('none@example.test','💌 Neuigkeiten für dich!')).HTML.includes('Ci vediamo!'),'Each participant receives the shared message in a localized email');

 const retentionEvent=await call('/api/events','POST',{...initial,organizer_email:'retention@example.test'});
 const retentionPath='/api/events/'+retentionEvent.public_token;
 const retentionToken=tokenFrom((await messageFor('retention@example.test')).Text);
 const retentionSlots=(await call(retentionPath)).slots;
 await call(retentionPath+'/votes','POST',{...v,email:'retention-voter@example.test',slots:[retentionSlots[0].id]});
 if(expectPdf)await call(retentionPath+'/votes','POST',{...v,email:'second-spanish-voter@example.test',slots:[retentionSlots[0].id]});
 await call(retentionPath+'/admin/'+retentionToken,'PUT',{status:'CONFIRMED',confirmed_slot_id:retentionSlots[0].id,locale:'it'});
 if(expectPdf){
  const first=await messageFor('retention-voter@example.test','🎉');
  const second=await messageFor('second-spanish-voter@example.test','🎉');
  const organizer=await messageFor('retention@example.test','🎉');
  check(first.Attachments[0].Checksums.SHA256===second.Attachments[0].Checksums.SHA256,'Same-language recipients share the same PDF');
  check(first.Attachments[0].Checksums.SHA256!==organizer.Attachments[0].Checksums.SHA256,'Organizer receives the PDF in their own language');
 }
 await queueDrained();
 await until(async()=>(await client.query('SELECT count(*) FROM confirmation_invitations')).rows[0].count==='0','Completed invitation PDF is retained');
 const {id:retentionId}= (await client.query('SELECT id FROM events WHERE public_token=$1',[retentionEvent.public_token])).rows[0];
 const {id:invitationId}=(await client.query("INSERT INTO confirmation_invitations (event_id,payload) VALUES ($1,'{}'::jsonb) RETURNING id",[retentionId])).rows[0];
 await client.query("INSERT INTO email_outbox (event_id,recipient,subject,body,html,available_at,invitation_id) VALUES ($1,'expired@example.test','Pending','Private text','Private HTML',now()+interval '1 day',$2)",[retentionId,invitationId]);
 for(const zone of [initial.timezone,'UTC'])await clearOfMidnight(zone);
 await client.query("UPDATE event_slots SET slot_date=(now() AT TIME ZONE 'Europe/Rome')::date-10 WHERE id=$1",[retentionSlots[0].id]);
 await client.query("UPDATE event_slots SET slot_date=(now() AT TIME ZONE 'Europe/Rome')::date-11 WHERE id=$1",[singleEvent.slots[0].id]);
 const statsCron=()=>fetch(api+'/api/cron/email',{headers:{Authorization:`Bearer ${cronSecret}`}});
 await client.query("INSERT INTO usage_metrics (day, action, outcome) VALUES ((now() AT TIME ZONE 'UTC')::date-10, 'old_metric', 'ok')");
 check((await statsCron()).ok,'Maintenance checks poll and observability retention');
 check((await call(retentionPath)).status==='CONFIRMED','Confirmed event remains visible on the entire tenth calendar day');
 check((await client.query("SELECT count(*) FROM usage_metrics WHERE action='old_metric'")).rows[0].count==='0','Usage counters rotate after ten UTC calendar days');
 await client.query("UPDATE event_slots SET slot_date=(now() AT TIME ZONE 'Europe/Rome')::date-11 WHERE id=$1",[retentionSlots[0].id]);
 const {id:unknownZone}=(await client.query("INSERT INTO events (public_token,admin_token,title,location,organizer_name,organizer_email,timezone) VALUES ('unknown-zone-public-0','unknown-zone-admin-00','Old','Roma','Ada','zone@example.test','Mars/City') RETURNING id")).rows[0];
 await client.query("INSERT INTO event_slots (event_id,slot_date,start_time) VALUES ($1,current_date-30,'20:00')",[unknownZone]);
 check((await fetch(api+'/api/cron/email',{headers:{Authorization:`Bearer ${cronSecret}`}})).ok,'Retention runs from the protected cron');
 check((await client.query('SELECT count(*) FROM events WHERE id=$1',[unknownZone])).rows[0].count==='0','A zone unknown to PostgreSQL falls back to UTC instead of blocking retention');
 await call(retentionPath,'GET',undefined,404);
 check((await call('/api/events/'+single.public_token)).slots.length===2,'Open event uses its latest proposal for retention');
 const remaining=(await client.query(`SELECT
   (SELECT count(*) FROM event_slots WHERE event_id=$1) AS slots,
   (SELECT count(*) FROM participants WHERE event_id=$1) AS participants,
   (SELECT count(*) FROM votes WHERE slot_id=$2) AS votes,
   (SELECT count(*) FROM email_outbox WHERE event_id=$1) AS mail,
   (SELECT count(*) FROM confirmation_invitations WHERE event_id=$1) AS invitations
 `,[retentionId,retentionSlots[0].id])).rows[0];
 assert.deepEqual(remaining,{slots:'0',participants:'0',votes:'0',mail:'0',invitations:'0'});
 check((await messagesFor('expired@example.test')).length===0,'Pending email for expired event was never sent');
 const flood=await call('/api/events','POST',{...initial,organizer_email:'flood@example.test'});
 const floodPath=`/api/events/${flood.public_token}/admin/${tokenFrom((await messageFor('flood@example.test')).Text)}`;
 let limited;
 for(let i=0;i<25&&!limited;i++){
  const response=await fetch(web+floodPath,{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({status:i%2?'OPEN':'CANCELLED',confirmed_slot_id:null,locale:'it'})});
  if(response.status===429)limited=await response.json();else assert.equal(response.status,200);
 }
 check(limited?.error==='rate_limited','Repeated status changes on one event are rate limited');
 console.log(`API, database and email checks passed (${checks} assertions).`);

 const browserEnv={...process.env};
 const runtimeDir=join(toolsDir,'browser-libs');
 try{await access(join(runtimeDir,'lib','libnspr4.so'));browserEnv.LD_LIBRARY_PATH=[join(runtimeDir,'lib'),browserEnv.LD_LIBRARY_PATH].filter(Boolean).join(':');}catch{}
 try{await access(join(runtimeDir,'fonts.conf'));browserEnv.FONTCONFIG_FILE ||= join(runtimeDir,'fonts.conf');}catch{}
 browser=await chromium.launch({headless:true,env:browserEnv,handleSIGINT:false,handleSIGTERM:false});
 const emailPreview=await newPage({viewport:{width:360,height:780}});
 await emailPreview.setContent(confirmation.HTML);
 check(await emailPreview.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Confirmation email fits a narrow screen');
 await emailPreview.close();
 const page=await newPage({locale:'it-IT',timezoneId:'Europe/Rome'});
 for(const [locale,code] of [['de-AT','de'],['ar-EG','ar']]){
  const visitor=await newPage({locale});
  await visitor.addInitScript(()=>new MutationObserver((_,observer)=>{if(document.body){window.firstPaint=[document.documentElement.lang,document.documentElement.dir];observer.disconnect();}}).observe(document,{childList:true,subtree:true}));
  await visitor.goto(web);await visitor.waitForSelector('header[data-ready="true"]');
  const dictionary=JSON.parse(await readFile(`locales/${code}.json`,'utf8'));
  await visitor.getByRole('heading',{name:dictionary.ui.create,exact:true}).waitFor();
  check((await visitor.evaluate(()=>window.firstPaint.join()))===`${code},${code==='ar'?'rtl':'ltr'}`&&await visitor.evaluate(()=>localStorage.getItem('locale'))===null,`First visit follows the ${locale} browser before the first paint without saving it`);
  await visitor.close();
 }
 await page.goto(web);
 await page.locator('footer').getByRole('link',{name:'Informativa sulla privacy'}).click();
 await page.getByRole('heading',{name:'Informativa sulla privacy'}).waitFor();
 check(await page.getByText(/Frizzi non vende mai/).isVisible()&&await page.getByText(/fine del decimo giorno/).isVisible()&&await page.getByText(/conteggi aggregati/).isVisible(),'Public privacy notice states commercial, retention and observability terms');
 await page.addInitScript(()=>{
   Object.defineProperty(navigator,'share',{value:async data=>localStorage.setItem('sharedEvent',JSON.stringify(data))});
 });
 const browserErrors=[];page.on('pageerror',error=>browserErrors.push(error.message));
 const unavailable=await newPage({locale:'it-IT'});
 await unavailable.route('**/api/events/*',route=>route.fulfill({status:502,contentType:'text/html',body:'<h1>Bad gateway</h1>'}));
 await unavailable.goto(web+retentionPath.replace('/api/events',''));
 await unavailable.getByText('Il servizio è temporaneamente indisponibile. Riprova tra poco.').waitFor();
 await unavailable.unroute('**/api/events/*');await unavailable.getByRole('button',{name:'Riprova',exact:true}).click();
 await unavailable.getByRole('link',{name:'Crea un evento',exact:true}).waitFor();
 check(await unavailable.getByRole('heading',{name:'Pagina non trovata'}).isVisible(),'Retry reloads in place and a missing event offers to create one');
 await unavailable.close();
 await page.goto(web);await page.waitForSelector('header[data-ready="true"]');await page.waitForSelector('input[name="title"]');
 const select=page.locator('header select');
 // Dictionaries other than Italian load on demand: wait until the page has switched.
 const chooseLanguage=async code=>{await select.selectOption(code);await page.waitForFunction(code=>document.documentElement.lang===code,code);};
 for(const locale of locales){
   const dictionary=JSON.parse(await readFile(`locales/${locale}.json`,'utf8'));
   await select.selectOption(locale);
   await page.getByRole('heading',{name:dictionary.ui.create,exact:true}).waitFor();
   check(await page.locator('html').getAttribute('dir')===(locale==='ar'?'rtl':'ltr'),`Direction for ${locale}`);
 }
 await chooseLanguage('ar');await page.reload();
 await page.waitForSelector('header[data-ready="true"]');
 check(await select.inputValue()==='ar','Language persists after reload');
 await chooseLanguage('it');
 const venue={label:'Caffè Florian, Piazza San Marco 57, Venezia',geo:{
  latitude:45.4336,longitude:12.33828,kind:'venue',attribution:'© OpenStreetMap contributors'
 }};
 const street={label:'Piazza San Marco, Venezia',geo:{
  latitude:45.434,longitude:12.338,kind:'approximate',attribution:'© OpenStreetMap contributors'
 }};
 await page.route('**/api/places/suggest',async route=>{
  assert.equal(route.request().postDataJSON().locale,'it');
  await route.fulfill({json:{enabled:true,suggestions:[venue,street]}});
 });
 await page.locator('[name="title"]').fill('Cena dal browser');
 const eventDescription='Una cena tra amici nel cuore di Venezia, con due chiacchiere e qualcosa di buono da condividere.';
 await page.locator('textarea[name="description"]').fill(eventDescription);
 const locationInput=page.getByRole('combobox',{name:'Dove ci troviamo? *'});
 await locationInput.fill('Caffè Florian Venezia');
 await page.getByRole('option').filter({hasText:'Posizione approssimativa'}).waitFor();
 await locationInput.press('ArrowDown');
 await locationInput.press('Enter');
 check(await locationInput.inputValue()===venue.label,'Keyboard selects and retains the correct venue');
 const dateInput=page.locator('[name="date-0"]');
 check(await dateInput.getAttribute('placeholder')==='GG/MM/AAAA','Italian date field spells day/month/year in Italian');
 await dateInput.fill('31/02/2030');
 check(await dateInput.evaluate(input=>!input.checkValidity()),'Localized date rejects an impossible day');
 await dateInput.blur();await page.getByText('Inserisci una data valida nel formato indicato.').waitFor();
 check(await dateInput.getAttribute('aria-invalid')==='true'&&(await dateInput.getAttribute('aria-describedby'))==='date-0-error','An impossible date is flagged with aria-invalid and a visible message');
 await dateInput.fill('31032030');
 await dateInput.blur();
 check(await dateInput.inputValue()==='31/03/2030','Compact keyboard entry is normalized to the selected date format');
 await chooseLanguage('de');
 check(await dateInput.inputValue()==='31.03.2030','Date field follows German formatting');
 await chooseLanguage('zh');
 check(await dateInput.inputValue()==='2030/03/31','Date field follows Chinese date order');
 await chooseLanguage('it');
 check((await page.locator('[name="start-0-minute"] option').allTextContents()).join(',')==='—,00,05,10,15,20,25,30,35,40,45,50,55','Minutes are offered in five-minute steps');
 await page.locator('[name="start-0-hour"]').selectOption('19');
 await page.locator('[name="start-0-minute"]').selectOption('30');
 await page.locator('[name="end-0-hour"]').selectOption('22');
 check(await page.locator('[name="end-0-minute"]').inputValue()==='00','An hour defaults to zero minutes');
 await page.locator('[name="end-0-hour"]').selectOption('');
 check(await page.locator('[name="end-0-minute"]').isDisabled(),'Optional end time can be cleared');
 await page.locator('[name="end-0-hour"]').selectOption('22');
 await page.locator('[name="end-0-minute"]').selectOption('45');
 await page.locator('[name="voting_deadline"]').fill('25/02/2030');
 await page.getByRole('button',{name:'Aggiungi una data'}).click();
 check(await page.locator('[name="date-1"]').inputValue()==='01/04/2030','New date defaults to the next calendar day across a month boundary');
 check(await page.locator('[name="date-1"]').evaluate(input=>input===document.activeElement),'Adding a date focuses its date field');
 check(await page.locator('[name="start-1-hour"]').inputValue()==='19' && await page.locator('[name="start-1-minute"]').inputValue()==='30','New date copies the start time');
 check(await page.locator('[name="end-1-hour"]').inputValue()==='22' && await page.locator('[name="end-1-minute"]').inputValue()==='45','New date copies the end time');
 await page.locator('[name="end-1-hour"]').selectOption('');
 check(await page.locator('[name="end-0-hour"]').inputValue()==='22','Changing the new row leaves the previous one intact');
 await page.locator('[name="organizer_name"]').fill('Browser Ada');await page.locator('[name="organizer_email"]').fill('browser@example.test');
 await page.locator('[name="end-0-hour"]').selectOption('18');await page.locator('[name="date-1"]').fill('31/03/2030');
 await page.getByRole('button',{name:'Crea evento',exact:true}).click();
 await page.getByRole('alert').filter({hasText:'Controlla i campi'}).waitFor();
 const endHour=page.locator('[name="end-0-hour"]'),secondDate=page.locator('[name="date-1"]');
 const describedText=async field=>page.locator('#'+await field.getAttribute('aria-describedby')).textContent();
 check(await endHour.getAttribute('aria-invalid')==='true'&&await describedText(endHour)==='L’orario di fine deve essere dopo l’inizio.'&&await endHour.evaluate(field=>field===document.activeElement),'An end time before the start is flagged on its own row and focused');
 check(await secondDate.getAttribute('aria-invalid')==='true'&&(await describedText(secondDate)).startsWith('Questa data è già nell’elenco')&&await page.locator('[name="date-0"]').getAttribute('aria-invalid')==='false','A repeated date is flagged on the later row only');
 await endHour.selectOption('22');await secondDate.fill('01/04/2030');
 check(await page.locator('.slot-error').count()===0&&await endHour.getAttribute('aria-invalid')===null,'Row errors clear as the rows are fixed');
 await page.getByRole('button',{name:'Crea evento',exact:true}).click();
 await page.getByRole('heading',{name:'Evento creato!',exact:true}).waitFor();
 check(await page.getByRole('heading',{name:'Evento creato!',exact:true}).evaluate(heading=>heading===document.activeElement),'The success heading takes focus after creating');
 const publicUrl=await page.locator('input[readonly]').inputValue();const browserToken=publicUrl.split('/').at(-1);
 await page.getByRole('button',{name:'Condividi',exact:true}).click();
 check(JSON.parse(await page.evaluate(()=>localStorage.getItem('sharedEvent'))).url===publicUrl,'Share opens the public event only');
 await page.getByRole('link',{name:'Crea un altro evento',exact:true}).click();
 await page.getByRole('heading',{name:'Crea il tuo evento',exact:true}).waitFor();
 check((await Promise.all(['title','location','organizer_name','organizer_email','date-0'].map(name=>page.locator(`[name="${name}"]`).inputValue()))).join('')===''&&await page.locator('[name="date-1"]').count()===0,'Create another starts from an empty form');
 await page.locator('[name="date-0"]').fill('31/02');await page.getByRole('button',{name:'Aggiungi una data'}).click();
 await page.getByRole('button',{name:/^Rimuovi/}).first().click();
 check(await page.locator('[name="date-0"]').inputValue()===''&&await page.locator('[name="date-1"]').count()===0,'Removing a date row leaves no stale draft in the next one');
 check(await page.getByRole('button',{name:'Aggiungi una data'}).evaluate(button=>button===document.activeElement),'Removing the second-last row moves focus to Add a date');
 const browserAdmin=tokenFrom((await messageFor('browser@example.test')).Text);
 const debugResponse=await fetch(`${api}/api/events/${browserToken}/admin/${browserAdmin}/tiebreak`,{
  method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({slot_id:randomUUID()})
 });
 check(debugResponse.status===409,'Invalid tiebreak is a traceable conflict');
 const requestId=debugResponse.headers.get('x-request-id');
 check(/^[a-f0-9-]{36}$/.test(requestId),'API returns a generated support reference');
 // The server names log files by its own UTC date; read them all so a run across midnight still sees every row.
 const logs=(await Promise.all((await readdir(logDir)).filter(name=>name.endsWith('.jsonl')).sort().map(name=>readFile(join(logDir,name),'utf8')))).join('\n');
 const rows=logs.split('\n').filter(line=>line.trim()).map(line=>JSON.parse(line));
 check(rows.some(entry=>entry.span?.request_id===requestId && entry.span?.route==='/api/events/{public}/admin/{secret}/tiebreak'),'Trace stores the normalized route and support reference');
 check(rows.some(entry=>entry.fields.event==='mail_deferred' && entry.fields.reason.startsWith('smtp_')),'A failed SMTP attempt identifies its failure class');
 check(![browserAdmin,browserToken,'browser@example.test','Cena dal browser'].some(value=>logs.includes(value)),'Logs exclude private route parameters and form content');
 await page.goto(publicUrl);
 const directions=page.getByRole('link',{name:'Indicazioni',exact:true});
 await directions.waitFor();
 check(new URL(await directions.getAttribute('href')).searchParams.get('destination')==='45.4336,12.33828','Directions use saved coordinates');
 const google=page.getByRole('link',{name:'Vedi su Google Maps',exact:true});
 check(new URL(await google.getAttribute('href')).searchParams.get('query')===venue.label,'Google Maps uses venue name and address');
 check((await call(`/api/events/${browserToken}`)).location_geo.attribution===venue.geo.attribution,'Location attribution survives saving');
 const desktopViewport=page.viewportSize();
 await page.setViewportSize({width:320,height:800});
 const detailsCard=page.locator('.event-header');
 const status=page.locator('.poll-status');
 check(await status.getAttribute('data-state')==='open'&&(await status.textContent()).trim()==='Sondaggio aperto'&&await status.locator('svg').count()===1,'Open poll has a clear status and icon');
 const partyImage=page.locator('.availability-party img');
 await partyImage.evaluate(img=>img.decode());
 check(await partyImage.evaluate(img=>img.naturalWidth>0&&img.getBoundingClientRect().height<=100),'Party illustration loads and stays compact on mobile');
 check(await page.locator('label.date-option .date-item-icon svg').count()===2,'Every selectable date has a calendar icon');
 check(await detailsCard.getByText(eventDescription,{exact:true}).isVisible()&&await detailsCard.getByText('Browser Ada',{exact:true}).isVisible(),'Description and organizer are readable on mobile');
 check(await detailsCard.evaluate(node=>node.scrollWidth<=node.clientWidth),'Venue details fit a narrow mobile screen');
 check(Math.abs((await directions.boundingBox()).y-(await google.boundingBox()).y)<=1,'Map actions stay on the same row on mobile');
 await mkdir('test-results',{recursive:true});
 await detailsCard.screenshot({path:'test-results/event-details.png'});
 await page.evaluate(()=>window.scrollTo(0,0));
 await page.screenshot({path:'test-results/availability-mobile.png'});
 await page.setViewportSize(desktopViewport);
 await page.locator('[name="participant_name"]').fill('Browser Guest');await page.locator('[name="participant_email"]').fill('browser-guest@example.test');
 const deadlineBanner=page.locator('.vote-deadline');
 check(await deadlineBanner.locator('time').getAttribute('datetime')==='2030-02-25','Voting deadline is prominently displayed with the correct date');
 check((await deadlineBanner.locator('time').textContent())==='lun 25 feb 2030','Public deadline spells out the month in the active language');
 await chooseLanguage('de');
 check((await deadlineBanner.locator('time').textContent())==='Mo., 25. Feb. 2030','Public deadline updates when the language changes');
 await chooseLanguage('it');
 const leaders=page.locator('.result-leaders');
 check(await leaders.count()===0,'No leading date before a vote');
 const traveller=await newPage({locale:'it-IT',timezoneId:'America/New_York'});
 await traveller.goto(publicUrl);await traveller.locator('label.date-option').first().waitFor();
 check((await traveller.locator('label.date-option').first().textContent()).includes('Nel tuo fuso orario: 13:30')&&await traveller.locator('.event-fact[title="Europe/Rome"]').textContent().then(text=>text.includes('Ora dell’Europa centrale')),'Guests elsewhere see their own time and a readable event zone');
 await traveller.close();
 const marchDay=page.locator('button[data-calendar-date="2030-03-31"]');
 const aprilDay=page.locator('button[data-calendar-date="2030-04-01"]');
 const checkboxes=page.locator('[name="dates"]');
 await marchDay.click();
 check(await checkboxes.first().isChecked(),'Calendar selection checks the corresponding box');
 await checkboxes.first().uncheck();
 check(await marchDay.getAttribute('aria-pressed')==='false','Unchecking a date clears its calendar selection');
 await marchDay.click();
 await checkboxes.last().check();
 await aprilDay.waitFor();
 check(await aprilDay.getAttribute('aria-pressed')==='true','Checkbox selects and reveals a date in another month');
 check(await page.locator('.event-calendar').getAttribute('aria-hidden')==='true'&&await aprilDay.getAttribute('tabindex')==='-1','The calendar mirrors the date list for pointer users only');
 await page.locator('.calendar-month button[aria-label="Mese precedente"]').click();
 check(await marchDay.getAttribute('aria-pressed')==='true','Selections survive month navigation');
 await checkboxes.last().uncheck();
 await page.evaluate(()=>{document.querySelector('.event-header').dataset.mounted='yes';document.addEventListener('click',()=>window.clickScroll=scrollY,{once:true,capture:true});});
 await page.getByRole('button',{name:'Invia disponibilità',exact:true}).click();
 const voteNotice=page.locator('form .vote-feedback .notice').filter({hasText:'Risposta salvata'});
 await voteNotice.waitFor();
 check(await page.locator('.feedback-area').count()===0&&await voteNotice.evaluate(notice=>notice.parentElement.nextElementSibling?.matches('button[type="submit"]')),'The vote notice sits right above the submit button, not at the top of the page');
 check(await page.evaluate(()=>document.querySelector('.event-header')?.dataset.mounted==='yes'&&window.clickScroll>0&&scrollY>0),'Saving a vote refreshes the results without remounting the page or jumping to the top');
 check(await page.evaluate(()=>document.activeElement?.classList.contains('notice'))&&await page.getByRole('list',{name:'Tutti i partecipanti'}).getByRole('listitem').count()===1,'The saved notice takes focus and participants are a named list');
 check((await page.locator('.result>span').allTextContents()).join('|')==='1 disponibile|0 disponibili','Vote counts use Italian singular and plural forms');
 await until(async()=>{
  const stats=await client.query("SELECT count(*) FROM usage_metrics WHERE (action, outcome) IN (('create_event','ok'),('event_location','suggested'),('vote_selection','both'),('share','native'),('browser_error','invalid_response'))");
  return stats.rows[0].count==='5';
 },'Server and browser events are counted by action and outcome');
 check(true,'Usage statistics capture real browser actions and proxy errors');
 await leaders.getByRole('heading',{name:'Data in testa'}).waitFor();
 check((await leaders.locator('time').evaluateAll(nodes=>nodes.map(node=>node.dateTime))).join(',')==='2030-03-31','The first positive vote identifies the sole leading date');
 await page.locator('[name="participant_name"]').fill('Browser Guest');await page.locator('[name="participant_email"]').fill('browser-guest@example.test');
 await page.getByRole('button',{name:'Invia disponibilità',exact:true}).click();
 const voteError=page.locator('form .vote-feedback .error');await voteError.waitFor();
 check(await page.locator('[name="participant_email"]').getAttribute('aria-describedby')===await voteError.getAttribute('id')&&(await voteError.textContent()).startsWith('Questa email ha già risposto'),'A vote error is shown by the button and linked to the email field');
 await page.reload();await page.locator('[name="participant_name"]').waitFor();
 await page.getByText('Hai perso il link privato?',{exact:true}).click();
 await page.locator('[name="recovery_email"]').fill('browser-guest@example.test');
 await page.getByRole('button',{name:'Reinvia il link',exact:true}).click();
 await page.getByRole('status').filter({hasText:'Se l’indirizzo è associato all’evento'}).waitFor();
 const browserEdit=tokenFrom((await messageFor('browser-guest@example.test')).Text);
 const editUrl=`${publicUrl}/${browserEdit}`;
 await page.goto(editUrl);
 await marchDay.waitFor();
 check(await marchDay.getAttribute('aria-pressed')==='true' && !(await checkboxes.last().isChecked()),'Private participant link restores the saved selections');
 await checkboxes.last().check();
 await page.getByRole('button',{name:'Salva modifiche',exact:true}).click();
 await leaders.getByRole('heading',{name:'Date a pari merito'}).waitFor();
 check((await leaders.locator('time').evaluateAll(nodes=>nodes.map(node=>node.dateTime))).join(',')==='2030-03-31,2030-04-01','The summary lists every date tied for first');
 await page.goto(web+'/'+single.public_token);
 const singleDates=(await call('/api/events/'+single.public_token)).slots;
 await page.locator(`button[data-calendar-date="${singleDates[0].date}"]`).click();
 await checkboxes.last().check();
 check(!(await checkboxes.first().isChecked()) && await page.locator(`button[data-calendar-date="${singleDates[1].date}"]`).getAttribute('aria-pressed')==='true'&&await checkboxes.first().getAttribute('type')==='radio','Single-choice mode is a radio group that replaces the previous selection');
 await page.goto(`${publicUrl}/admin/${browserAdmin}`);
 await page.getByText(/Più disponibilità/).first().waitFor();
 await page.context().grantPermissions(['clipboard-read','clipboard-write'],{origin:web});
 const adminLinkBox=page.getByRole('region',{name:'Il tuo link di gestione'});
 await adminLinkBox.getByRole('button',{name:'Copia link'}).click();
 check(await adminLinkBox.getByRole('textbox').inputValue()===`${publicUrl}/admin/${browserAdmin}`&&await page.evaluate(()=>navigator.clipboard.readText())===`${publicUrl}/admin/${browserAdmin}`&&(await adminLinkBox.textContent()).includes('Tienilo privato'),'The organizer can see and copy their private management link');
 await page.getByText('Scrivi ai partecipanti',{exact:true}).click();
 const messageBox=page.getByRole('textbox',{name:'Messaggio',exact:true});
 await messageBox.fill('Ci vediamo domani!');
 await messageBox.selectText();
 await page.getByRole('button',{name:'Grassetto',exact:true}).click();
 check(await messageBox.inputValue()==='<b>Ci vediamo domani!</b>','Organizer can format selected text');
 check(await page.getByRole('region',{name:'Anteprima'}).locator('b').textContent()==='Ci vediamo domani!','The message preview renders the allowed formatting');
 await page.getByRole('button',{name:'Invia messaggio',exact:true}).click();
 const messageDialog=page.getByRole('dialog',{name:'Inviare il messaggio?'});
 await messageDialog.waitFor();
 check((await messageDialog.textContent()).includes('1 persona riceverà questo messaggio via email.'),'Sending a message asks for confirmation with the recipient count');
 await messageDialog.getByRole('button',{name:'Invia messaggio',exact:true}).click();
 await page.getByRole('status').filter({hasText:'1 email in coda'}).waitFor();
 check((await messageFor('browser-guest@example.test','💌 Una novità per te!')).HTML.includes('<b>Ci vediamo domani!</b>'),'Admin composer delivers the formatted message');
 await page.locator('.calendar-month button[aria-label="Mese successivo"]').click();
 await aprilDay.click();
 check(await page.evaluate(()=>document.activeElement?.id.startsWith('confirm-')) && (await call('/api/events/'+browserToken)).status==='OPEN','Admin selection focuses the confirmation without changing event status');
 check(await page.getByText(/Più disponibilità/).count()===2,'Admin highlights tied dates');
 await page.getByRole('button',{name:'Modifica i dettagli',exact:true}).click();
 check(await page.locator('[name="voting_deadline"]').inputValue()==='25/02/2030','Admin can see configured deadline in the active date format');
 check(await page.locator('[name="start-0-hour"]').inputValue()==='19' && await page.locator('[name="start-0-minute"]').inputValue()==='30','Stored time loads in the editor');
 await page.locator('[name="location"]').fill('Bologna');await page.getByRole('button',{name:'Salva modifiche',exact:true}).click();
 await page.locator('.feedback-area .notice').filter({hasText:'Modifiche salvate'}).waitFor();
 const editedVenue=await call(`/api/events/${browserToken}`);
 check(editedVenue.location==='Bologna' && editedVenue.location_geo===null,'Manual editing clears stale coordinates');
 await leaders.getByRole('button',{name:/Voto di spareggio · dom 31 mar 2030/}).click();
 await leaders.getByRole('heading',{name:'Data scelta allo spareggio'}).waitFor();
 check(await leaders.locator('time').count()===1&&await leaders.locator('time').getAttribute('datetime')==='2030-03-31','Deciding vote leaves one winner');
 const [red,green,blue]=(await leaders.evaluate(el=>getComputedStyle(el).backgroundColor)).match(/[\d.]+/g).map(Number);
 const high=Math.max(red,green,blue),low=Math.min(red,green,blue),hue=high===blue&&high>low?240+60*(red-green)/(high-low):-1;
 check(hue>=250&&hue<=300&&(high+low)/510>=0.85,`Leaderboard uses a light violet background (hue ${Math.round(hue)})`);
 check(await page.evaluate(async()=>{
  const fonts=await document.fonts.load('600 14px Inter');
  return fonts.some(font=>font.family==='Inter')&&getComputedStyle(document.querySelector('h1')).fontFamily.startsWith('Inter');
 }),'The site loads the local Inter font');
 check(await page.evaluate(()=>{
  const preload=document.querySelector('link[rel="preload"][as="font"]');
  return !!preload&&[...document.fonts].some(font=>font.family==='Inter'&&font.status==='loaded'&&font.unicodeRange.includes('U+400-52F'))&&new URL(preload.href).pathname.endsWith('.woff2');
 }),'The Latin and Cyrillic Inter subset is preloaded and declares its unicode-range');
 const offGrid=await call('/api/events','POST',{...initial,organizer_email:'offgrid@example.test',slots:[{date:'2030-02-03',start:'19:07',end:null}]});
 await page.goto(`${web}/${offGrid.public_token}/admin/${tokenFrom((await messageFor('offgrid@example.test')).Text)}`);
 await page.locator('.responses>p').filter({hasText:'Ancora nessuna risposta. Condividi il link per raccogliere le disponibilità.'}).waitFor();
 await page.getByRole('button',{name:'Modifica i dettagli',exact:true}).click();
 check(await page.locator('[name="start-0-minute"]').inputValue()==='07'&&await page.locator('[name="start-0-minute"] option').count()===14,'Editing keeps a saved minute that is off the five-minute grid');
 await page.goto(publicUrl);
 await page.getByText('Voto decisivo dell’organizzatore',{exact:true}).waitFor();
 check(await page.getByRole('button',{name:/spareggio/i}).count()===0,'Participants see the deciding vote without organizer controls');
 await page.goto(`${publicUrl}/admin/${browserAdmin}`);
 await page.getByRole('button',{name:/^Conferma questa data · /}).first().waitFor();
 check(new Set(await page.getByRole('button',{name:/^Conferma questa data · /}).evaluateAll(buttons=>buttons.map(button=>button.getAttribute('aria-label')))).size===2,'Each confirm button names its date');
 const confirmDate=page.getByRole('button',{name:/^Conferma questa data · /}).first();
 const confirmDialog=page.getByRole('dialog',{name:'Confermare questa data?'});
 await confirmDate.click();await confirmDialog.waitFor();await page.keyboard.press('Escape');await confirmDialog.waitFor({state:'hidden'});
 check((await call('/api/events/'+browserToken)).status==='OPEN'&&await confirmDate.evaluate(button=>button===document.activeElement),'Confirming a date needs the dialog; Escape keeps the poll open and returns focus');
 await confirmDate.click();await confirmDialog.getByRole('button',{name:'Conferma e avvisa tutti',exact:true}).click();await page.locator('.event-confirmed').waitFor();
 check(await status.getAttribute('data-state')==='confirmed'&&(await status.textContent()).trim()==='Evento confermato','Confirmation has its own status badge');
 check(await page.getByText('Voto decisivo dell’organizzatore',{exact:true}).isVisible(),'Confirmed winner keeps its deciding-vote badge');
 const calendarLink=page.getByRole('link',{name:'Aggiungi al calendario',exact:true});
 check((await calendarLink.getAttribute('href')).endsWith('/calendar.ics'),'Calendar shown in confirmed event');
 check(await leaders.count()===0 && await deadlineBanner.count()===0,'Confirming the event hides provisional leaders and the polling deadline');
 await page.setViewportSize({width:320,height:844});
 await page.goto(editUrl);
 const confirmedDay=page.locator('span[data-calendar-date="2030-03-31"].confirmed.marked');
 await confirmedDay.waitFor();
 check(await page.locator('button[data-calendar-date]').count()===0,'Closed participant calendar shows saved and confirmed dates in read-only mode');
 check(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),'Calendar layout fits narrow mobile screens');
 await call(`/api/events/${single.public_token}/votes`,'POST',{name:'Organizer too',email:'SINGLE@example.test',locale:'de',slots:[]});
 const singleVoter=tokenFrom((await messageFor('single@example.test','✅')).Text);
 await client.query("UPDATE events SET voting_deadline='2000-01-01' WHERE public_token=$1",[single.public_token]);
 await page.goto(web+'/'+single.public_token);
 await status.filter({hasText:'Sondaggio chiuso'}).waitFor();
 check(await status.getAttribute('data-state')==='closed'&&await status.locator('svg').count()===1,'An expired poll is labeled closed with an icon');
 await call(`/api/events/${single.public_token}/admin/${singleAdmin}`,'PUT',{status:'CANCELLED',confirmed_slot_id:null,locale:'it'});
 await messageFor('single@example.test','📌');await queueDrained();
 const organizerCancel=(await messagesFor('single@example.test')).filter(m=>m.Subject.startsWith('📌'));
 const organizerCancelText=(await messageFor('single@example.test','📌')).Text;
 check(organizerCancel.length===1&&tokenFrom(organizerCancelText)===singleAdmin&&organizerCancelText.includes(`/${single.public_token}/${singleVoter}`),'An organizer who voted gets one notification with both private links');
 await page.reload();
 await status.filter({hasText:'Evento annullato'}).waitFor();
 check(await status.getAttribute('data-state')==='cancelled'&&await partyImage.count()===0,'Cancelled event has a distinct badge and no celebration art');
 await page.setViewportSize({width:390,height:844});
 await page.goto(web);await page.waitForSelector('[name="title"]');
 check(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),'Mobile page overflow');
 await mkdir('test-results',{recursive:true});await page.screenshot({path:'test-results/mobile.png',fullPage:true});
 await page.setViewportSize({width:1440,height:1000});await page.screenshot({path:'test-results/desktop.png',fullPage:true});
 check(browserErrors.length===0,`Browser errors: ${browserErrors.join('; ')}`);
 console.log(`Browser checks passed. Total ${checks} assertions; screenshots in test-results/.`);
}catch(error){await fail(error);}
finally{await cleanup();}
