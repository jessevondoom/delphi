//! The investigator's shell (`specs/116` §8.1, AC-10 and AC-11): a throwaway copy of the project, always in the OS sandbox, with
//! no network, and a project that is byte-identical afterwards. The red-team reviews this file with the slice.

use crate::approval::{Answer, Tier as ApprovalTier};
use crate::engine::Status;
use crate::exec::{ExecConfig, ExecEnv, ExecError, SandboxMode};
use crate::model::Step;
use crate::testkit::{setup_with_exec, Fx};
use crate::worktree::tests::{commit, index_and_refs, init_repo, tree_bytes};
use delphi_sandbox::Sandbox;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PROMPT: &str = "Find the commit that broke @project/app/src/lib.rs";

/// Where this machine keeps a `git` that runs in the sandbox, as `[exec] toolchain` and `PATH`. On macOS `/usr/bin/git` is a shim
/// that asks the developer tools where the real one is, which the sandbox does not let it do; the real one runs. An operator
/// sets the same two keys.
fn with_git(c: &mut ExecConfig) {
    if !cfg!(target_os = "macos") {
        return;
    }
    for root in ["/Applications/Xcode.app/Contents/Developer", "/Library/Developer/CommandLineTools", "/opt/homebrew", "/usr/local"] {
        let bin = Path::new(root).join(if root.contains("Developer") { "usr/bin" } else { "bin" });
        if bin.join("git").exists() {
            c.toolchain.push(PathBuf::from(root));
            c.env.insert("PATH".into(), format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()));
            return;
        }
    }
}

/// The fixture project as a repository of four commits; the third introduces `BUG`. Returns the four commits, oldest first.
fn history(proj: &Path, home: &Path) -> Vec<String> {
    init_repo(proj, home);
    let mut shas = vec![commit_head(proj, home)];
    for (n, body) in [(2, "pub fn needle() {}\npub fn two() {}\n"), (3, "pub fn needle() {}\npub fn two() {}\n// BUG\n"), (4, "pub fn needle() {}\npub fn two() {}\n// BUG\npub fn four() {}\n")] {
        fs::write(proj.join("src/lib.rs"), body).unwrap();
        shas.push(commit(proj, home, &format!("commit {n}")));
    }
    shas
}

fn commit_head(proj: &Path, home: &Path) -> String {
    fs::write(proj.join("README"), "fixture\n").unwrap();
    commit(proj, home, "readme")
}

fn fixture(steps: Vec<Step>, mode: SandboxMode) -> (Fx, Vec<String>) {
    let f = setup_with_exec(steps, 5.0, |_| {}, |c| {
        c.sandbox = mode;
        with_git(c);
    });
    let shas = history(&f.root.join("proj"), &f.root.join("git-home"));
    (f, shas)
}

fn events(f: &Fx, kind: &str) -> Vec<Value> {
    f.m.replay().unwrap().into_iter().filter(|e| e.kind == kind).map(|e| e.payload).collect()
}

fn results(f: &Fx) -> Vec<(String, bool)> {
    events(f, "tool.result").into_iter().map(|p| (p["content"].as_str().unwrap_or("").to_string(), p["is_error"].as_bool().unwrap_or(false))).collect()
}

fn offered(f: &Fx) -> Vec<String> {
    f.model.requests()[0].body["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

fn copies(f: &Fx) -> Vec<String> {
    fs::read_dir(f.root.join("workspaces/w/worktrees/.investigations")).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
}

async fn wait_for_card(f: &Fx) -> crate::approval::Request {
    for _ in 0..500 {
        if let Some(r) = f.engine.broker.pending().into_iter().next() {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no approval card appeared");
}

/// AC-10: an investigator span runs `git bisect` over a fixture repository and names the commit; afterwards the project's
/// working tree, index and refs are byte-identical and the span's worktree is gone.
#[tokio::test]
async fn an_investigator_bisects_in_a_throwaway_copy_and_the_project_is_byte_identical_afterwards() {
    if Sandbox::detect().is_err() {
        return;
    }
    let bisect = "git bisect start HEAD HEAD~3 >/dev/null && git bisect run sh -c '! grep -q BUG src/lib.rs' 2>&1 | grep 'first bad commit'; echo scratch > notes.tmp; git log --oneline | wc -l";
    let (f, shas) = fixture(vec![Step::tool("c1", "shell", json!({"command": bisect})), Step::tool("c2", "run_command", json!({"argv": ["git", "status", "--short"]})), Step::text("The third commit introduced it.")], SandboxMode::Prefer);
    let proj = f.root.join("proj");
    // The operator's own uncommitted work is in the project while the investigation runs.
    fs::write(proj.join("vendor/x.js"), "// the operator is editing this\n").unwrap();
    let before = tree_bytes(&proj);
    assert_eq!(f.engine.run_task(&f.m, "investigator", PROMPT).await.unwrap().status, Status::Done);

    let tools = offered(&f);
    assert!(tools.contains(&"shell".to_string()) && tools.contains(&"run_command".to_string()), "{tools:?}");
    let got = results(&f);
    assert!(!got[0].1 && got[0].0.contains(&format!("{} is the first bad commit", shas[2])), "the bisect named the commit: {}", got[0].0);
    assert!(got[0].0.contains("sandboxed, no network"), "{}", got[0].0);
    assert!(!got[1].1 && got[1].0.contains("notes.tmp"), "the second command ran in the same copy: {}", got[1].0);
    assert!(events(&f, "approval.requested").is_empty(), "under an active sandbox the builder is not asked, so the investigator is not");
    // The copy: made once, under the workspace's worktree folder, and gone when the span ended.
    let (made, gone) = (events(&f, "investigation.worktree_created"), events(&f, "investigation.worktree_removed"));
    assert_eq!((made.len(), gone.len()), (1, 1));
    assert_eq!((made[0]["project"].as_str(), made[0]["base_sha"].as_str()), (Some("app"), Some(shas[3].as_str())), "detached at the project's head");
    assert_eq!(gone[0]["gone"], true);
    assert!(copies(&f).is_empty(), "the span's worktree is gone: {:?}", copies(&f));
    assert!(!f.root.join("workspaces/w/scratch").join(made[0]["key"].as_str().unwrap()).exists(), "and its scratch space");
    // Not a task worktree: no branch, nothing for a merge gate to find.
    assert!(events(&f, "worktree.created").is_empty() && events(&f, "provenance.stamp").is_empty());
    assert!(f.engine.exec.known_worktree(events_task(&f).as_str()).is_none());
    // The project: byte-identical. The spec names the working tree, the index and the refs; every other byte is held too.
    let after = tree_bytes(&proj);
    assert_eq!(index_and_refs(&after), index_and_refs(&before), "index and refs");
    assert!(after.keys().all(|k| !k.contains("delphi/")) && !after.contains_key(".git/worktrees/"), "no branch and no linked worktree was ever made in the project");
    assert_eq!(after, before, "working tree, index, refs and everything else in the project");
}

fn events_task(f: &Fx) -> String {
    f.m.replay().unwrap().into_iter().find(|e| e.kind == "task.created").and_then(|e| e.task_id).unwrap()
}

/// AC-11: from the investigator's shell, a push, a network request and a write outside its worktree are each refused, by the
/// text rule where the command says what it is and by the sandbox where it does not.
#[tokio::test]
async fn a_push_a_network_request_and_a_write_outside_the_copy_are_each_refused() {
    if Sandbox::detect().is_err() {
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    // The steps need the project's path, which the fixture makes, so they are handed to the model after it.
    let (f, _shas) = fixture(vec![], SandboxMode::Prefer);
    let proj = fs::canonicalize(f.root.join("proj")).unwrap();
    let p = proj.display();
    let hidden_push = format!("g=git; $g push {p} HEAD:refs/heads/planted 2>&1; echo \"push rc=$?\"; $g push file://{p} HEAD:refs/heads/planted2 2>&1; echo \"push2 rc=$?\"");
    let net = format!("(exec 3<>/dev/tcp/127.0.0.1/{port}) 2>&1 && echo CONN\"\"ECTED; echo \"net rc=$?\"; curl -s -m 3 http://127.0.0.1:{port}/ ; echo \"curl rc=$?\"");
    let outside = format!("echo x > {p}/planted.txt; echo \"write rc=$?\"; echo x >> {p}/.git/config; echo \"gitcfg rc=$?\"; cat {p}/src/lib.rs; echo \"read rc=$?\"; echo x > .git; echo \"gitfile rc=$?\"; echo x > ../../../../planted-ws; echo \"ws rc=$?\"; cat .git");
    let script = std::sync::Mutex::new(std::collections::VecDeque::from(vec![
        Step::tool("c1", "shell", json!({"command": "ls && git push origin HEAD"})),
        Step::tool("c2", "run_command", json!({"argv": ["/usr/bin/git", "-C", ".", "fetch", "origin"]})),
        Step::tool("c3", "shell", json!({"command": "sh -c 'git pull'; git remote add evil https://evil.example/x.git"})),
        Step::tool("c4", "shell", json!({"command": hidden_push})),
        Step::tool("c5", "shell", json!({"command": net})),
        Step::tool("c6", "shell", json!({"command": outside})),
        Step::text("Nothing left the copy."),
    ]));
    f.model.respond_with(move |_| script.lock().unwrap().pop_front().unwrap_or_else(|| Step::text("done")));
    let before = tree_bytes(&proj);
    assert_eq!(f.engine.run_task(&f.m, "investigator", PROMPT).await.unwrap().status, Status::Done);
    let got = results(&f);
    // Said plainly: refused by the text rule, before anything runs.
    for (n, word) in [(0, "git push"), (1, "git fetch"), (2, "git pull")] {
        assert!(got[n].1 && got[n].0.contains("refused by policy") && got[n].0.contains(word), "call {n}: {}", got[n].0);
    }
    assert_eq!(events(&f, "action.intent").len(), 3, "the three refused calls never started a process");
    // Hidden from the text rule: it runs, and the sandbox refuses it.
    assert!(got[3].0.contains("push rc=") && !got[3].0.contains("push rc=0") && !got[3].0.contains("push2 rc=0"), "a push to the project by path fails: {}", got[3].0);
    assert!(!got[4].0.contains("CONNECTED") && !got[4].0.contains("curl rc=0"), "no network, not even loopback: {}", got[4].0);
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock), "nothing reached the listener");
    let o = &got[5].0;
    for rc in ["write rc=", "gitcfg rc=", "read rc=", "gitfile rc=", "ws rc="] {
        assert!(o.contains(rc) && !o.contains(&format!("{rc}0")), "{rc}: {o}");
    }
    assert!(!o.contains("pub fn needle"), "the project's files cannot even be read: {o}");
    assert!(o.contains("gitdir: "), "the copy's pointer file is intact: {o}");
    assert!(!proj.join("planted.txt").exists() && !f.root.join("workspaces/w/planted-ws").exists() && !f.root.join("workspaces/planted-ws").exists());
    assert!(copies(&f).is_empty());
    assert_eq!(tree_bytes(&proj), before, "the project is byte-identical: no ref, no object, no config line, no file");
}

/// §8.1 items 2 and 5: with the OS sandbox not in use for builders (`[exec] sandbox = "off"`), the builder is asked about each
/// command, so the investigator is; and its command still runs in the sandbox, which the card says.
#[tokio::test]
async fn the_investigator_is_asked_what_the_builder_is_asked_and_still_runs_sandboxed() {
    if Sandbox::detect().is_err() {
        return;
    }
    let (f, _) = fixture(vec![Step::tool("c1", "run_command", json!({"argv": ["/bin/sh", "-c", "echo hi > out.txt; cat out.txt"]})), Step::tool("c2", "shell", json!({"command": "rm -rf /"})), Step::text("done")], SandboxMode::Off);
    f.engine.broker.set_mode(crate::approval::Mode::MissionAuto);
    let proj = f.root.join("proj");
    let before = tree_bytes(&proj);
    let task = { let (e, m) = (f.engine.clone(), f.m.clone()); tokio::spawn(async move { e.run_task(&m, "investigator", PROMPT).await.unwrap().status }) };
    let card = wait_for_card(&f).await;
    assert_eq!((card.tool.as_str(), card.tier), ("run_command", ApprovalTier::Amber), "even in the most permissive mode, as for a builder");
    assert_eq!((card.payload["sandboxed"].clone(), card.payload["cwd"].clone()), (json!(true), json!("@investigation")));
    assert!(card.summary.contains("echo hi") && card.summary.contains("throwaway worktree, sandboxed with no network") && !card.summary.contains("not sandboxed"), "{}", card.summary);
    assert!(events(&f, "action.intent").is_empty(), "nothing runs before the answer");
    f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Allow, "console-1").unwrap();
    // A destructive line is the Red card it is for a builder; denied, it does not run.
    let card = wait_for_card(&f).await;
    assert_eq!((card.tool.as_str(), card.tier), ("shell", ApprovalTier::Red));
    f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "no".into() }, "console-1").unwrap();
    assert_eq!(task.await.unwrap(), Status::Done);
    let got = results(&f);
    assert!(!got[0].1 && got[0].0.contains("hi") && got[0].0.contains("sandboxed, no network"), "{}", got[0].0);
    assert!(got[1].1, "{}", got[1].0);
    assert_eq!(events(&f, "action.intent").len(), 1);
    assert!(copies(&f).is_empty());
    assert_eq!(tree_bytes(&proj), before);
}

/// A span that ends by escalating (here: its turn budget) leaves no copy behind either, and a deny rule of the operator's
/// refuses the investigator what it refuses the builder.
#[tokio::test]
async fn an_escalated_span_leaves_no_copy_and_the_operators_deny_rules_apply() {
    if Sandbox::detect().is_err() {
        return;
    }
    let mut steps: Vec<Step> = vec![Step::tool("c0", "shell", json!({"command": "echo one > a.txt && cargo publish"}))];
    steps.extend((1..60).map(|n| Step::tool(&format!("c{n}"), "run_command", json!({"argv": ["/bin/echo", "again"]}))));
    let f = setup_with_exec(steps, 50.0, |_| {}, |c| {
        c.sandbox = SandboxMode::Prefer;
        c.deny = vec!["cargo publish*".into()];
        with_git(c);
    });
    history(&f.root.join("proj"), &f.root.join("git-home"));
    let before = tree_bytes(&f.root.join("proj"));
    let out = f.engine.run_task(&f.m, "investigator", PROMPT).await.unwrap();
    assert!(matches!(out.status, Status::Escalated(_)), "{:?}", out.status);
    let got = results(&f);
    assert!(got[0].1 && got[0].0.contains("deny rule"), "{}", got[0].0);
    assert_eq!(events(&f, "investigation.worktree_created").len(), 1, "one copy for the whole span");
    assert!(copies(&f).is_empty(), "{:?}", copies(&f));
    assert_eq!(tree_bytes(&f.root.join("proj")), before);
}

/// Who holds which shell. The scope alone does not make a Builder: only the builder's manifest writes into the worktree, and
/// only a Builder runs in a task's worktree, keeps a harness CLI's own tools there, or may use an operator authorization.
#[test]
fn only_the_builder_builds_and_only_the_investigator_investigates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agents");
    let (mut build, mut investigate) = (vec![], vec![]);
    for e in fs::read_dir(&root).unwrap().flatten() {
        let Ok(text) = fs::read_to_string(e.path().join("AGENT.md")) else { continue };
        let role = e.file_name().to_string_lossy().into_owned();
        let m = crate::workspace::AgentManifest::parse(&text).unwrap_or_else(|e| panic!("{role}: {e}"));
        if crate::engine::builds(&m) {
            build.push(role.clone());
        }
        if crate::engine::investigates(&m) {
            investigate.push(role);
        }
    }
    assert_eq!((build, investigate), (vec!["builder".to_string()], vec!["investigator".to_string()]));
    for (file, call) in [("harness_run.rs", "let builder = wt.is_some() && crate::engine::builds(&sc.manifest);"), ("authorization_use.rs", "pre_refusal(crate::engine::builds(&sc.manifest)")] {
        let src = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)).unwrap();
        assert!(src.contains(call), "{file}: the scope alone must not make a Builder there");
    }
    let engine = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine.rs")).unwrap();
    assert_eq!(engine.matches("\"shell-worktree-scoped\"").count(), 1, "engine.rs reads the scope through `builds` and `investigates` (and SHELL_SCOPE), nowhere else");
}

/// The exec layer on its own: always the sandbox, whatever the configured mode, and nothing without one.
#[test]
fn an_investigation_command_never_runs_without_the_os_sandbox() {
    let d = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(d.path()).unwrap();
    let project = crate::worktree::tests::project(&root);
    let before = tree_bytes(&project);
    let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // No sandbox on the machine: the copy can be made (the daemon's git), and nothing runs in it.
    let none = ExecEnv::new(&root.join("ws0"), ExecConfig { sandbox: SandboxMode::Off, ..Default::default() }, None);
    let guard = none.open_investigation("i_none", &project).unwrap();
    assert_eq!(none.run_investigation(&guard.inv, &argv(&["/bin/echo", "hi"]), None).unwrap_err(), ExecError::NoSandbox);
    let gone = guard.inv.root.clone();
    drop(guard);
    assert!(!gone.exists(), "dropping the guard deletes the copy");
    let Ok(sb) = Sandbox::detect() else { return };
    // `sandbox = "off"`: a Builder's command would run unsandboxed; an investigation's is sandboxed all the same.
    let mut cfg = ExecConfig { sandbox: SandboxMode::Off, ..Default::default() };
    with_git(&mut cfg);
    let env = ExecEnv::new(&root.join("ws"), cfg, Some(sb));
    assert!(env.active_sandbox().is_none());
    let guard = env.open_investigation("i_one", &project).unwrap();
    assert!(matches!(env.open_investigation("i_one", &project), Err(crate::worktree::GitError::Exists(_))), "one copy a key");
    let script = format!("echo x > {}/planted; echo \"rc=$?\"; echo made > here.txt; git checkout -q --detach HEAD 2>&1; git status --short", project.display());
    let r = env.run_investigation(&guard.inv, &argv(&["/bin/sh", "-c", &script]), None).unwrap();
    assert!(r.sandboxed && r.outcome.stdout.contains("rc=") && !r.outcome.stdout.contains("rc=0"), "{:?}", r.outcome);
    assert!(r.outcome.stdout.contains("here.txt"), "git works inside the copy: {:?}", r.outcome);
    assert_eq!(r.changed, ["here.txt"]);
    assert!(!project.join("planted").exists());
    // A second span's copy sweeps nothing that is live, and a leftover goes.
    fs::create_dir_all(env.worktrees.investigations_root().join("i_leftover/tree")).unwrap();
    let second = env.open_investigation("i_two", &project).unwrap();
    assert!(guard.inv.root.exists() && second.inv.root.exists() && !env.worktrees.investigations_root().join("i_leftover").exists());
    drop((guard, second));
    assert!(fs::read_dir(env.worktrees.investigations_root()).unwrap().next().is_none());
    assert_eq!(tree_bytes(&project), before);
    // The text rule, in the spellings a deny rule catches.
    for line in ["git push", "git push origin main", "/usr/bin/git -C . push", "GIT -c x=y fetch --all", "ls && git pull", "sh -c 'git clone https://x.example/y'", "echo $(git ls-remote origin)", "git remote add x y", "git submodule update --init"] {
        assert!(ExecEnv::investigation_denied_line(line).is_some(), "{line}");
    }
    for line in ["git bisect start HEAD HEAD~3", "git log --oneline", "git checkout --detach HEAD~2", "git commit -m 'push the fix'", "cargo test", "grep -r fetch src"] {
        assert!(ExecEnv::investigation_denied_line(line).is_none(), "{line}");
    }
    assert!(ExecEnv::investigation_denied(&argv(&["git", "--no-pager", "push"])).is_some() && ExecEnv::investigation_denied(&argv(&["git", "stash", "push"])).is_none());
}
