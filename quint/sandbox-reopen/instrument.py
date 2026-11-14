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
p.add_argument('--plant',choices=['none','no_kept','no_running_check','no_revive_check','no_guard','leak_guard','boot_reclaims','keep_failed_daemon','no_revived_report','status_no_wait','no_fallback','count_unattached'],default='none')
a=p.parse_args()
source=ROOT/'sdk/js/native/src'
originals={str(f.relative_to(source)):f.read_text() for f in source.rglob('*.rs')}
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
edit('workspace.rs','    let (state, revived) = place(args, &project)?;','    let (state, revived) = place(args, &project)?;\n    crate::replay::placed()?;')
edit('workspace.rs','    Ok((crate::sandbox::start(args, project)?, revived))','    crate::replay::placed()?;\n    Ok((crate::sandbox::start(args, project)?, revived))')
edit('workspace.rs','                        Ok(_) => {}','                        Ok(_) => { crate::replay::placed()?; }')
edit('sandbox.rs','Path::new(PROCESSES).join(&container_name)','Path::new(&crate::replay::processes()).join(&container_name)')
edit('discovery/revive.rs','    let (saved, lost): (Vec<_>, Vec<_>) = records.into_iter().partition(|saved| saved.overlay);','    let trace_target = crate::replay::is_target(&layout.directory);\n    let (saved, lost): (Vec<_>, Vec<_>) = records.into_iter().partition(|saved| saved.overlay);')
edit('discovery/revive.rs','    let restart = match revive_rules::next(found, saved.len()) {','    let decision = revive_rules::next(found, saved.len());\n    if trace_target { crate::replay::checked(match decision { Next::Use => "use", Next::Restart => "restart", Next::Reclaim => "none" }); }\n    let restart = match decision {')
edit('discovery/revive.rs','        Err(refusal) => {\n            let reason = refusal.message;','        Err(refusal) => {\n            if trace_target { crate::replay::target_failed(&refusal.message); }\n            let reason = refusal.message;')
edit('discovery/revive.rs','    let label = if restart { "restarted" } else { "running" };','    if trace_target { crate::replay::target_ok(); }\n    let label = if restart { "restarted" } else { "running" };')
edit('discovery/revive.rs','    let (control, instance) = dial.first()?;','    crate::replay::transport_fault(&mut dial.secret);\n    let (control, instance) = dial.first()?;')
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
            if outcome=="listener" { fs::write(sandbox.layout.log(),b"invalid listen line\\n").expect("listener fault"); }
        }
'''
edit('core_process.rs',needle,hook+needle)
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
if a.plant=='no_revived_report': edit('workspace.rs','        fields.push(("revived".to_owned(), SdkValue::List(revived)));','        let _ = revived;')
if a.plant=='status_no_wait': edit('registry.rs','if matches!(project, ProjectGuard::Free)\n                && crate::discovery::gone_container(&refusal).is_some()','if false\n                && crate::discovery::gone_container(&refusal).is_some()')
if a.plant=='no_fallback': edit('workspace.rs','                                    revived.push(item)','                                    return Err(crate::job::refused("Failed", "target revive failed"));')
if a.plant=='count_unattached': edit('discovery/revive.rs','revive_rules::base(saved.root, legacy_head)?','revive_rules::base(saved.root, true)?')
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
