//! Explicit, bounded system-Git operations.
pub mod backend;
mod browser;
mod signing;
use backend::Git;
use std::{ffi::OsString,io::{Read,Write},path::{Path,PathBuf,Component}};
use std::os::unix::ffi::{OsStrExt,OsStringExt};
fn args(values:&[&str])->Vec<OsString>{values.iter().map(OsString::from).collect()}
fn emit(out:&mut dyn Write,b:&[u8])->Result<(),String>{out.write_all(b).map_err(|e|e.to_string())}
pub(super) fn escape(b:&[u8])->String{b.iter().map(|&b|if (32..127).contains(&b)||b==b'\n'{(b as char).to_string()}else{format!("\\x{b:02x}")}).collect()}
fn rendered(out:&mut dyn Write,b:&[u8])->Result<(),String>{emit(out,escape(b).as_bytes())}
fn paths(values:&[OsString],cwd:&Path,root:&Path)->Result<Vec<OsString>,String>{
 if values.is_empty(){return Err("explicit file paths required".into())}
 values.iter().map(|v|{let p=cwd.join(v);let mut clean=PathBuf::new();for c in p.components(){match c{Component::ParentDir=>{clean.pop();},Component::CurDir=>{},x=>clean.push(x.as_os_str())}}
 let relative=clean.strip_prefix(root).map_err(|_|"path is outside worktree")?;
 if relative.as_os_str().is_empty(){return Err("directory operands are unsupported".into())}
 match std::fs::symlink_metadata(&clean){Ok(m)if m.is_file()||m.file_type().is_symlink()=>{},Ok(_)=>return Err("only explicit files and symlinks are supported".into()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string())}
 Ok(relative.as_os_str().to_owned())}).collect()
}
fn nul(paths:&[OsString])->Vec<u8>{let mut b=Vec::new();for p in paths{b.extend_from_slice(p.as_bytes());b.push(0)}b}
fn resolve(g:&Git,value:&OsString)->Result<OsString,String>{if value.as_bytes().starts_with(b"-"){return Err("option-like revision refused".into())}let mut a=args(&["rev-parse","--verify","--end-of-options"]);let mut v=value.clone();v.push("^{commit}");a.push(v);let b=g.run(&a,b"")?;let b=b.strip_suffix(b"\n").unwrap_or(&b);if !matches!(b.len(),40|64)||!b.iter().all(u8::is_ascii_hexdigit){return Err("invalid object ID".into())}Ok(OsString::from_vec(b.to_vec()))}
pub(super) fn inspect(g:&Git,op:&str,rest:&[OsString])->Result<Vec<u8>,String>{
 let a=match op{
 "status"=>args(&["status","--porcelain=v2","-z","--branch","--untracked-files=all","--ignore-submodules=all"]),
 "diff"=>{let mut a=args(&["diff","--no-ext-diff","--no-textconv","--ignore-submodules=all","--no-color"]);a.extend_from_slice(rest);a},
 "log"=>{let limit=if rest.is_empty(){50}else if rest.len()==2&&rest[0]=="--limit"{rest[1].to_str().ok_or("invalid limit")?.parse::<usize>().map_err(|_|"invalid limit")?}else{return Err("log accepts --limit N".into())};if !(1..=1000).contains(&limit){return Err("limit must be 1..1000".into())}args(&["log","--no-show-signature","--no-decorate","--no-color","--format=%H%x09%an%x09%ae%x09%aI%x09%s",&format!("-{limit}")])},
 "show"=>{if rest.len()!=1{return Err("show requires one commit".into())}let id=resolve(g,&rest[0])?;let mut a=args(&["show","--no-show-signature","--no-ext-diff","--no-textconv","--ignore-submodules=all","--no-color"]);a.push(id);a.push("--".into());a},
 "branches"=>args(&["for-each-ref","--format=%(objectname)%09%(refname)%09%(HEAD)","refs/heads/","refs/remotes/"]),
 "stashes"=>args(&["reflog","show","--no-show-signature","--format=%H%x09%gd%x09%gs","refs/stash"]),
 _=>return Err("unknown inspection".into())};g.run(&a,b"")
}
pub fn execute(all:&[OsString],_input:&mut dyn Read,out:&mut dyn Write,_terminal:bool)->Result<u8,String>{
 let mut executable=PathBuf::from("/usr/bin/git");let mut cwd=std::env::current_dir().map_err(|e|e.to_string())?;let mut i=0;
 while i<all.len(){if all[i]=="--git-bin"||all[i]=="-C"{let flag=&all[i];i+=1;let value=all.get(i).ok_or("missing global option value")?;if flag=="--git-bin"{executable=value.into()}else{cwd=std::fs::canonicalize(cwd.join(value)).map_err(|e|e.to_string())?}i+=1}else{break}}
 let Some(op)=all.get(i).and_then(|x|x.to_str())else{emit(out,b"Usage: terms git [--git-bin ABS] [-C PATH] status|diff|log|show|branches|stashes|identity|browse|stage|unstage|commit\n")?;return Ok(0)};let rest=&all[i+1..];
 let g=backend::discover(executable,cwd.clone())?;g.preflight(false,&[])?;
 match op{
 "identity" if rest.is_empty()=>{let id=g.identity()?;emit(out,id.summary.as_bytes())?;Ok(0)},
 "browse" if rest.is_empty()=>browser::execute(&g),
 "status" if rest.is_empty()||rest==[OsString::from("--null")]=>{let b=inspect(&g,op,&[])?;if rest.is_empty(){let human=b.split(|&x|x==0).map(escape).collect::<Vec<_>>().join("\n");emit(out,human.as_bytes())?;emit(out,b"\n")?}else{emit(out,&b)?}Ok(0)},
 "diff"=>{let mut staged=false;let mut pos=Vec::new();let mut options=true;for v in rest{if options&&v=="--"{options=false}else if options&&v=="--staged"{staged=true}else if options&&v.as_bytes().starts_with(b"-"){return Err("unknown diff option".into())}else{pos.push(v.clone())}}let mut a=Vec::new();if staged{a.push("--cached".into())}a.push("--".into());if !pos.is_empty(){a.extend(paths(&pos,&cwd,&g.root)?)}rendered(out,&inspect(&g,op,&a)?)?;Ok(0)},
 "log"|"show"=>{rendered(out,&inspect(&g,op,rest)?)?;Ok(0)},
 "branches"|"stashes" if rest.is_empty()=>{rendered(out,&inspect(&g,op,rest)?)?;Ok(0)},
 "stage"|"unstage"=>{let mut apply=false;let mut pos=Vec::new();let mut options=true;for v in rest{if options&&v=="--"{options=false}else if options&&v=="--apply"{apply=true}else if options&&v.as_bytes().starts_with(b"-"){return Err("unknown write option".into())}else{pos.push(v.clone())}}let p=paths(&pos,&cwd,&g.root)?;g.preflight(true,&p)?;let snapshot=g.snapshot(&p)?;emit(out,format!("{} {}\nRepository: {}\nHooks disabled.\n",if apply{"Applying"}else{"Preview"},op,escape(g.root.as_os_str().as_bytes())).as_bytes())?;for v in &p{emit(out,format!("  {}\n",escape(v.as_bytes())).as_bytes())?}if !apply{return Ok(0)}if snapshot!=g.snapshot(&p)?{return Err("repository changed; retry preview".into())}g.preflight(true,&p)?;
 let a=if op=="stage"{args(&["add","--pathspec-from-file=-","--pathspec-file-nul"])}else if g.run(&args(&["rev-parse","--verify","HEAD"]),b"").is_ok(){args(&["restore","--staged","--source=HEAD","--pathspec-from-file=-","--pathspec-file-nul"])}else{args(&["update-index","--force-remove","-z","--stdin"])};rendered(out,&g.run(&a,&nul(&p))?)?;Ok(0)},
 "commit"=>commit(&g,rest,out),
 _=>Err("unknown Git operation or invalid arguments".into())
 }
}
fn commit(g:&Git,rest:&[OsString],out:&mut dyn Write)->Result<u8,String>{let(mut apply,mut message)=(false,None);let mut i=0;while i<rest.len(){if rest[i]=="--apply"{apply=true}else if rest[i]=="-m"||rest[i]=="--message-file"{if message.is_some(){return Err("one message source required".into())}let file=rest[i]=="--message-file";i+=1;let v=rest.get(i).ok_or("missing message")?;message=Some(if file{let m=std::fs::symlink_metadata(v).map_err(|e|e.to_string())?;if !m.is_file()||m.len()>1024*1024{return Err("message must be a regular file <=1 MiB".into())}std::fs::read(v).map_err(|e|e.to_string())?}else{v.as_bytes().to_vec()})}else{return Err("unknown commit option".into())}i+=1}let message=message.ok_or("explicit message required")?;if message.len()>1024*1024||message.iter().all(u8::is_ascii_whitespace)||message.contains(&0){return Err("invalid or empty message".into())}g.preflight(true,&[])?;if g.run(&args(&["diff","--cached","--name-only","-z","--no-ext-diff","--no-textconv"]),b"")?.is_empty(){return Ok(1)}let id=g.identity()?;let sign=signing::policy(g)?;let snapshot=g.snapshot(&[])?;
 emit(out,format!("{} commit\nRepository: {}\n{}\n{}\nHooks disabled.\nMessage:\n{}\n",if apply{"Applying"}else{"Preview"},escape(g.root.as_os_str().as_bytes()),id.summary,sign.summary,escape(&message)).as_bytes())?;rendered(out,&inspect(g,"diff",&args(&["--cached"]))?)?;if !apply{return Ok(0)}if snapshot!=g.snapshot(&[])?||id.summary!=g.identity()?.summary||sign.summary!=signing::policy(g)?.summary{return Err("repository or identity/signing configuration changed".into())}g.preflight(true,&[])?;
 let mut env=sign.env;env.extend([("GIT_AUTHOR_NAME".into(),id.author_name.into()),("GIT_AUTHOR_EMAIL".into(),id.author_email.into()),("GIT_COMMITTER_NAME".into(),id.committer_name.into()),("GIT_COMMITTER_EMAIL".into(),id.committer_email.into())]);let mut a=Vec::new(); let mut flags=Vec::new(); let mut iter=sign.args.into_iter(); while let Some(v)=iter.next(){if v=="-c"{a.push(v);a.push(iter.next().ok_or("missing signer config")?)}else{flags.push(v)}} a.extend(args(&["commit","--file=-","--cleanup=verbatim","--no-verify","--no-post-rewrite"]));a.extend(flags);rendered(out,&g.run_with_env(&a,&message,&env)?)?;let oid=g.run(&args(&["rev-parse","--verify","HEAD"]),b"")?;emit(out,&oid)?;Ok(0)}
