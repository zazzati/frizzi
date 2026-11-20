import { mkdir, writeFile, chmod } from 'node:fs/promises';
import { accessSync, statSync, constants } from 'node:fs';
import { homedir } from 'node:os';
import { join, delimiter } from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

export const toolsDir=join(homedir(),'.cache','frizzi');
const executable=path=>{try{accessSync(path,constants.X_OK);return statSync(path).isFile();}catch{return false;}};
const onPath=name=>(process.env.PATH||'').split(delimiter).filter(Boolean).map(dir=>join(dir,name)).find(executable);
// Resolution order: $MAILPIT, a mailpit binary on PATH, then the checksum-verified download in toolsDir.
// This is a live binding: after setupTools() resolves, importers see the final path.
export let mailpitPath=process.env.MAILPIT||onPath('mailpit')||join(toolsDir,'mailpit');
export async function setupTools(){
 if(process.env.MAILPIT&&!executable(process.env.MAILPIT))throw new Error(`MAILPIT is not an executable file: ${process.env.MAILPIT}`);
 if(executable(mailpitPath))return mailpitPath;
 const os={linux:'linux',darwin:'darwin'}[process.platform],arch={x64:'amd64',arm64:'arm64'}[process.arch];
 if(!os||!arch)throw new Error(`No Mailpit download for ${process.platform}-${process.arch}: install Mailpit and put it on PATH or set MAILPIT, or use Docker Compose.`);
 await mkdir(toolsDir,{recursive:true});
 const version='v1.34.0';
 const base=`https://github.com/axllent/mailpit/releases/download/${version}`;
 const archive=`mailpit-${os}-${arch}.tar.gz`;
 // The release publishes no checksums.txt, so the SHA-256 of each archive is pinned here (matches GitHub's asset digests).
 const expected={
  'linux-amd64':'af236b42f171b1c74f0ac5ff444f44d36f2f2ecc0134f5a2f1d4e307e56a0d2c',
  'linux-arm64':'179f3fe9642edb3a123edc613003e45641869c5b2728c2076c7bef1c3e09dd01',
  'darwin-amd64':'a497582d4e9f9a5d0eea84f8622530533dce0797cf8a4952bd7a59487e5d9c7c',
  'darwin-arm64':'32933e0bd17e9738f19ab2eb5ec4d50e183448c9f29ba97e762d418a0f72e854'
 }[`${os}-${arch}`];
 const response=await fetch(`${base}/${archive}`);if(!response.ok)throw new Error('Mailpit download failed');
 const blob=Buffer.from(await response.arrayBuffer());
 if(createHash('sha256').update(blob).digest('hex')!==expected)throw new Error('Mailpit checksum mismatch');
 const tarPath=join(toolsDir,archive);await writeFile(tarPath,blob);
 mailpitPath=join(toolsDir,'mailpit');
 execFileSync('tar',['-xzf',tarPath,'-C',toolsDir,'mailpit']);await chmod(mailpitPath,0o755);
 console.log(`Mailpit ${version} installed for ${os}-${arch}.`);
 return mailpitPath;
}
if(process.argv[1]===new URL(import.meta.url).pathname)await setupTools();
