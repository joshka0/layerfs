#!/usr/bin/env python3
"""Build exact first-party test copies, checkpoints, and explicit code plants."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent
p=argparse.ArgumentParser()
p.add_argument('--plant',choices=['none','no_kept','no_running_check','no_revive_check','no_guard','leak_guard','boot_reclaims','keep_failed_daemon','no_revived_report','status_no_wait','no_fallback','count_unattached','no_lock','no_holder_name','no_stale_check','lost_exit_diagnostic'],default='none')
a=p.parse_args()
source=ROOT/'sdk/js/native/src'
originals={str(f.relative_to(source)):f.read_text() for f in source.rglob('*.rs')}
originals['daemon_cli.rs']=(ROOT/'core/crates/layerfs-daemon/src/application/cli.rs').read_text()
originals['daemon_failure.rs']=(ROOT/'core/crates/layerfs-daemon/src/application/failure.rs').read_text()
originals['core_process.rs']=(ROOT/'core/crates/layerfs-sandbox/src/backend/process.rs').read_text()
files=dict(originals)
def edit(path,old,new):
 assert files[path].count(old)==1,(path,old,files[path].count(old))
 files[path]=files[path].replace(old,new)
# Real acquisition; a separate process probes its kernel ownership.
edit('discovery/lock.rs','        Ok(file) => Ok(Guard {\n            file,\n            directory: directory.to_owned(),\n        }),','        Ok(file) => { crate::replay::held(directory); Ok(Guard { file, directory: directory.to_owned() }) },')
# Read-only listing and status observations stay outside the project guard.
edit('workspace.rs','        match observed {\n            Ok(()) => items.push(item),','        crate::replay::observed(&value, observed.is_ok());\n        match observed {\n            Ok(()) => items.push(item),')
edit('registry.rs','''        {
            refusal
        }
        other => return other,
    };
    let _placing = match crate::discovery::guard''','''        {
            crate::replay::status_observed(args, false);
            refusal
        }
        other => { crate::replay::status_observed(args, true); return other; },
    };
    let _placing = match crate::discovery::guard''')
# Only new ordinary placement is outside the model. A failed target revive
# must execute the real fallback start, mount, and revived answer construction.
edit('workspace.rs','    let state = place(args, &project, revived)?;','    let state = place(args, &project, revived)?;\n    crate::replay::placed()?;')
edit('workspace.rs','    crate::sandbox::start(args, project)\n}','    crate::replay::placed()?;\n    crate::sandbox::start(args, project)\n}')
edit('workspace.rs','                        Ok(_) => {}','                        Ok(_) => { crate::replay::placed()?; }')
edit('sandbox.rs','Path::new(PROCESSES).join(&container_name)','Path::new(&crate::replay::processes()).join(&container_name)')
edit('discovery/revive.rs','    let (saved, lost): (Vec<_>, Vec<_>) = records.into_iter().partition(|saved| saved.overlay);','    let trace_target = crate::replay::is_target(&layout.directory);\n    let (saved, lost): (Vec<_>, Vec<_>) = records.into_iter().partition(|saved| saved.overlay);')
edit('discovery/revive.rs','    let restart = match revive_rules::next(found, saved.len()) {','    let decision = revive_rules::next(found, saved.len());\n    if trace_target && !matches!(decision, Next::Use) { crate::replay::checked(match decision { Next::Use => "use", Next::Restart => "restart", Next::Reclaim => "none" }); }\n    let restart = match decision {')
edit('discovery/revive.rs','        Err(refusal) => {\n            let reason = refusal.message;','        Err(refusal) => {\n            if trace_target { crate::replay::target_failed(&refusal.message); }\n            let reason = refusal.message;')
edit('discovery/revive.rs','    let state = match started {','    if trace_target && !restart { crate::replay::checked(if started.as_ref().err().is_some_and(|r| r.code == "StaleSandbox") { "stale" } else { "use" }); }\n    let state = match started {')
edit('workspace.rs','            Err(refusal) => match crate::discovery::gone_container(&refusal) {','            Err(refusal) => { if refusal.code == "StaleSandbox" { crate::replay::checked("stale"); } match crate::discovery::gone_container(&refusal) {')
edit('workspace.rs','                None => return Err(refusal),\n            },','                None => return Err(refusal),\n            } },')
edit('discovery/revive.rs','    let label = if restart { "restarted" } else { "running" };','    if trace_target { crate::replay::target_ok(); }\n    let label = if restart { "restarted" } else { "running" };')
edit('discovery/revive.rs','    let (control, instance) = dial.first()?;','    crate::replay::transport_fault(&mut dial);\n    let (control, instance) = dial.first()?;')
edit('sandbox/reclaim.rs','    if stopped {\n        match crate::discovery::runtime(&value)?.stopped().delete()','    crate::replay::checked("reclaim");\n    if stopped {\n        match crate::discovery::runtime(&value)?.stopped().delete()')
edit('sandbox/reclaim.rs','        return Err(refused(\n            KEPT,','        crate::replay::kept();\n        return Err(refused(\n            KEPT,')
edit('sandbox/reclaim.rs','    if text(&value, "container")? != container {','    if text(&value, "container")? != container {\n        crate::replay::checked("none");')
# Pause inside the real process backend after spawn, before listener wait.
# The library has no dependency on the driver. Its test-only checkpoint uses
# the actor's stdin/stdout and preserves the original wait duration.
needle='        let deadline = Instant::now() + wait;\n        loop {\n            match sandbox.listen_line()'
hook='''        if std::env::var_os("REOPEN_ACTOR").is_some()
            && std::env::var("REOPEN_ROOT").ok().is_some_and(|root| sandbox.layout.directory == Path::new(&root).join("runtime")) {
            println!("spawned pid:{}@{}", sandbox.pid, sandbox.start);
            std::io::stdout().flush().expect("spawn checkpoint flush");
            let mut command=String::new();
            assert!(std::io::stdin().read_line(&mut command).expect("spawn checkpoint input")>0,"scheduler disconnected");
            let words:Vec<_>=command.split_whitespace().collect();
            assert_eq!(words[0],"go","spawn checkpoint command");
            let outcome=words.get(1).copied().unwrap_or("ok");
            std::env::set_var("REOPEN_FINISH_OUTCOME",outcome);
            std::env::set_var("REOPEN_CHILD_ID",format!("pid:{}@{}",sandbox.pid,sandbox.start));
            if let Some(placed)=words.get(2) { std::env::set_var("REOPEN_PLACED",placed); }
            if let Some(saved)=words.get(3) { std::env::set_var("REOPEN_SAVED",saved); }
            if let Some(stopped)=words.get(4) { std::env::set_var("REOPEN_STOPPED",stopped); }
            if outcome=="listener" { fs::write(sandbox.layout.log(),b"invalid listen line\\n").expect("listener fault"); }
        }
'''
edit('core_process.rs',needle,hook+needle)
edit('core_process.rs','        let start = match start_time(pid) {',"""        if std::env::var_os("REOPEN_FAST_REJECT").is_some() && std::env::var("REOPEN_ACTOR").as_deref()==Ok("mount") && std::env::var("REOPEN_ROOT").ok().is_some_and(|r| layout.directory==Path::new(&r).join("runtime")) {
            let until=Instant::now()+Duration::from_secs(3);
            while matches!(start_time(pid),Ok(Some(_))) { assert!(Instant::now()<until,"rejected child did not exit"); thread::sleep(Duration::from_millis(2)); }
        }
        let start = match start_time(pid) {""")
edit('discovery/revive.rs','    save_sandbox(&directory, &state, args, secret, peer, instance)?;','    save_sandbox(&directory, &state, args, secret, peer, instance)?;\n    crate::replay::after_published();')

# The daemon executes its real folder admission between these file checkpoints.
edit('daemon_cli.rs','    let _folder = super::folder::hold(Path::new(&path))?;',
    '    let replay_root = replay_before(Path::new(&path));\n    let _folder = super::folder::hold(Path::new(&path))?;\n    if let Some(root) = replay_root { std::fs::write(root.join(format!("admission-{}.admitted",std::process::id())),b"admitted").expect("admission checkpoint"); }')
files['daemon_cli.rs'] += r"""
fn replay_before(config:&std::path::Path)->Option<std::path::PathBuf> {
 if std::env::var_os("REOPEN_SCHEDULE_ADMISSION").is_none() { return None; }
 let root=std::path::PathBuf::from(std::env::var("REOPEN_ROOT").ok()?);
 if config != root.join("runtime/config/daemon.setup") { return None; }
 assert!(root.starts_with(std::env::temp_dir()),"temporary admission fixture");
 let pid=std::process::id();
 std::fs::write(root.join(format!("admission-{pid}.ready")),b"ready").expect("admission ready");
 let until=std::time::Instant::now()+std::time::Duration::from_secs(30);
 while !root.join(format!("admission-{pid}.go")).exists() {
  assert!(std::time::Instant::now()<until,"admission scheduler did not release child");
  std::thread::sleep(std::time::Duration::from_millis(2));
 }
 Some(root)
}
"""

if a.plant=='lost_exit_diagnostic':
 edit('core_process.rs','Ok(None) => return Err(fail(StartPhase::Spawn, None, exited(&layout))),','Ok(None) => return Err(fail(StartPhase::Spawn,None,io::Error::other("daemon exited before its identity was read"))),')
 edit('core_process.rs','return Err(fail(StartPhase::Spawn, None, exited(&layout)))','return Err(fail(StartPhase::Spawn,None,io::Error::other("daemon exited before its identity was read")))')
if a.plant=='no_lock': edit('daemon_cli.rs','    let _folder = super::folder::hold(Path::new(&path))?;','    let _ = &path;')
if a.plant=='no_holder_name': edit('daemon_failure.rs','"layerfs-daemon: the Sandbox folder is held by daemon pid {pid}"','"layerfs-daemon: the Sandbox folder is held by a daemon"')
if a.plant=='no_stale_check': edit('pool.rs','self.hello(Some(self.instance), false)?','self.hello(None, false)?')
if a.plant=='no_kept': edit('sandbox/reclaim.rs','    if crate::discovery::holds_overlays(args, id)? {','    if false {')
if a.plant=='no_guard': edit('discovery/lock.rs','    while unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {','    while false {')
if a.plant=='leak_guard':
 for path,old in [('discovery/revive.rs','    let _placing = guard(&project_dir(args)?)?;'),('workspace.rs','    let _placing = crate::discovery::guard(&project)?;')]:
  edit(path,old,old.replace('= ','= std::mem::ManuallyDrop::new(',1).replace('?;','?);'))
if a.plant=='boot_reclaims': edit('discovery/revive.rs','        Next::Reclaim => {\n            let reason','        Next::Reclaim => {\n            fs::remove_dir_all(&layout.directory).map_err(failed)?;\n            fs::remove_dir_all(&directory).map_err(failed)?;\n            let reason')
if a.plant in ['no_running_check','no_revive_check']: edit('discovery/revive.rs','    let found = match process::Sandbox::reopen(layout.clone(), pid, start) {','    let found = match Err::<process::Sandbox,_>(std::io::Error::from(std::io::ErrorKind::NotFound)) {')
if a.plant=='no_running_check': edit('core_process.rs','        if start_time(pid).map_err(prepare)? == Some(start) {','        if false {')
if a.plant=='keep_failed_daemon':
 original=originals['discovery/revive.rs']
 start=original.index('    match daemon.stop(Duration::from_secs(5)) {')
 end=original.index('\n}\n\n/// The handshake',start)
 edit('discovery/revive.rs',original[start:end],'    let _ = (directory, value, pid, start, daemon);\n    "the planted bug leaves the spawned daemon running".to_owned()')
if a.plant=='no_revived_report':
 edit('workspace.rs','        if !revived.is_empty() {','        if false {')
 edit('workspace.rs','        fields.push(("revived".to_owned(), SdkValue::List(revived.clone())));','        let _ = revived;')
if a.plant=='status_no_wait': edit('registry.rs','if matches!(project, ProjectGuard::Free)\n                && crate::discovery::gone_container(&refusal).is_some()','if false\n                && crate::discovery::gone_container(&refusal).is_some()')
if a.plant=='no_fallback': edit('workspace.rs','                                    revived.push(item)','                                    return Err(crate::job::refused("Failed", "target revive failed"));')
if a.plant=='count_unattached':
 edit('discovery/revive.rs','revive_rules::base(saved.root)?','match revive_rules::base(saved.root) { Ok(root) => root, Err(_) => crate::direct::planted_branch_root(args, branch).map_err(|r| r.message)? }')
 files['direct.rs'] += '\npub(crate) fn planted_branch_root(\n    args: &SdkValue,\n    branch: layerfs_history::BranchId,\n) -> Result<[u8; 32], SdkRefusal> {\n    let service = service(args)?;\n    let snapshot = service\n        .store\n        .published(|history| history.branch_snapshot(branch))\n        .map_err(failed)?\n        .ok_or_else(|| refused("NoSuchBranch", "the saved Workspace\'s Branch is gone"))?;\n    Ok(snapshot.effective_root.to_bytes())\n}\n\n'
for child in ['lock','revive','revive_rules','sandbox']: edit('discovery.rs',f'mod {child};',f'#[path = "discovery/{child}.rs"] mod {child};')
edit('sandbox.rs','mod reclaim;','#[path = "sandbox/reclaim.rs"] mod reclaim;')
manifest={'plant':a.plant,'source':{},'compiled_source':{}}
diffs=[]
for path,text in files.items():
 out=HERE/'generated/src'/path;out.parent.mkdir(parents=True,exist_ok=True)
 if not out.exists() or out.read_text()!=text:out.write_text(text)
 manifest['source'][path]=hashlib.sha256(originals[path].encode()).hexdigest()
 manifest['compiled_source'][path]=hashlib.sha256(text.encode()).hexdigest()
 diffs.extend(difflib.unified_diff(originals[path].splitlines(True),text.splitlines(True),fromfile='production/'+path,tofile='compiled/'+path))
(HERE/'generated/manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(HERE/'generated/instrumentation.diff').write_text(''.join(diffs))
