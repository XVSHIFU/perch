import {test} from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,mkdtempSync,mkdirSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,basename} from 'node:path';
const parserUrl=new URL('../verification/p1/pi/node_modules/@earendil-works/pi-coding-agent/dist/core/package-manager.js',import.meta.url);
const parser=process.env.PERCH_TEST_INSTALLED_PI==='1'&&existsSync(parserUrl)?await import(parserUrl.href):null;

test('installed Pi parser applies package file rule precedence',{skip:!parser&&'Optional integration check: set PERCH_TEST_INSTALLED_PI=1 with the Pi fixture installed'},()=>{
 const {DefaultPackageManager}=parser;
 const root=mkdtempSync(join(tmpdir(),'perch-file-rules-'));
 try{
  mkdirSync(join(root,'prompts'));
  writeFileSync(join(root,'package.json'),JSON.stringify({name:'fixture',version:'1.0.0',pi:{prompts:['prompts']}}));
  for(const name of ['keep','skip','restore'])writeFileSync(join(root,'prompts',`${name}.md`),`# ${name}`);
  // Exercise the installed parser directly; no provider, process or model call.
  const manager=Object.create(DefaultPackageManager.prototype);
  const enabled=rules=>{
   const target=new Map();
   manager.applyPackageFilter(root,rules,'prompts',target,{source:'fixture',scope:'user',origin:'package'});
   return [...target.entries()].filter(([,item])=>item.enabled).map(([path])=>basename(path)).sort();
  };
  assert.deepEqual(enabled(['prompts/*.md','!prompts/skip.md','!prompts/restore.md','+prompts/restore.md']),['keep.md','restore.md']);
  assert.deepEqual(enabled(['+prompts/restore.md','-prompts/restore.md','prompts/*.md']),['keep.md','skip.md']);
  assert.deepEqual(enabled([]),[]);
 }finally{rmSync(root,{recursive:true,force:true});}
});
