//! Running a span through the Claude CLI (`specs/009` §7B): launch, bridge every permission request through
//! Delphi's rules, meter the subscription tokens live, and record what happened.
//!
//! The CLI runs its own loop, so a harness span has no per-turn events: it has `harness.started`, one
//! `harness.tool` per tool the CLI used, a final text block and turn, and one `agent.usage` (reported by
//! the CLI, `cost_model = subscription`). A span interrupted by the daemon dying is not resumable; the
//! task begins again from its saved request.

use crate::approval::{Need, Tier as ApprovalTier};
use crate::engine::{Engine, SpanCtx};
use crate::exec::{self, GitGuard, Verdict};
use crate::harness::{cli_env, Cli, Decision, Event, Launch, Permission, Session, Tools};
use crate::mission::{EngineError, LiveRedactor};
use crate::tools::{ActionKind, Classified, ToolTier};
use crate::worktree::Worktree;
use delphi_adapter::config::Endpoint;
use delphi_adapter::ipr::Ipr;
use delphi_core::hub::DeltaKind;
use delphi_core::provenance::{gate, Action, Gate, OutputCheck, Tier, Zone};
use delphi_core::usage::UsagePayload;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// `p` (absolute, or relative to `root`) resolved lexically and through symlinks, if it stays inside
/// `root` and passes through no `.git` segment. New files resolve through their nearest existing parent.
pub(crate) fn inside(root: &Path, p: &str) -> Option<PathBuf> {
    let root = std::fs::canonicalize(root).ok()?;
    let joined = if Path::new(p).is_absolute() { PathBuf::from(p) } else { root.join(p) };
    // Walk the path one component at a time, resolving symlinks as we go, so `link/..` means the parent of
    // what `link` points to (as the kernel sees it), never the parent of the link's own directory.
    let mut real = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                real.pop();
            }
            Component::CurDir => {}
            other => {
                real.push(other.as_os_str());
                if real.symlink_metadata().is_ok() {
                    real = std::fs::canonicalize(&real).ok()?; // a dangling link fails closed
                }
            }
        }
    }
    if !real.starts_with(&root) {
        return None;
    }
    if real.strip_prefix(&root).ok()?.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")) {
        return None;
    }
    Some(real)
}

fn path_of(input: &Value) -> Option<&str> {
    ["file_path", "path", "notebook_path"].iter().find_map(|k| input[k].as_str())
}

fn summarize(name: &str, input: &Value) -> String {
    let s = match name {
        "Bash" => input["command"].as_str().unwrap_or("").to_string(),
        _ => path_of(input).map(String::from).or_else(|| input["pattern"].as_str().map(String::from)).or_else(|| input["url"].as_str().map(String::from)).unwrap_or_default(),
    };
    s.chars().take(200).collect()
}

/// Why a harness span did not finish, and whether the same span may be tried by another method (an API
/// endpoint of the family) without repeating a side effect.
#[derive(Debug)]
pub(crate) struct HarnessErr {
    pub reason: String,
    pub retryable: bool,
}

impl HarnessErr {
    fn retry(reason: impl Into<String>) -> HarnessErr {
        HarnessErr { reason: reason.into(), retryable: true }
    }
}

/// What the CLI needs to know about Delphi's logical paths and its own situation.
fn harness_note(builder: bool) -> &'static str {
    if builder {
        "<harness_note>You are running inside Delphi. Your working directory is this task's git worktree, a checkout of the project at its current commit. Paths written as @worktree/x or @project/<name>/x in the task mean the file x in your working directory (the project's own files are at the top of it, for example src/lib.rs). Make the change by editing files here with your tools, run the checks you need, and do not commit: Delphi commits, verifies and asks the operator to merge. When the change is made, reply with a short summary of what you changed.</harness_note>"
    } else {
        "<harness_note>You are running inside Delphi with no tools and no file access. Everything you need is quoted in the task. Answer from it, and say so if something you need is missing.</harness_note>"
    }
}

impl Engine {
    // Which method serves a span (a harness, or the API turn by turn) is decided by the resolver in
    // `engine.rs` (`route_harness`, `has_api_route`), from capabilities and the pool, not from the family.

    /// One tool permission request, decided by the same rules as Delphi's own tools.
    async fn harness_decide(&self, sc: &mut SpanCtx, wt: &Option<Worktree>, p: &Permission) -> Result<Decision, EngineError> {
        let deny = |why: &str| Ok(Decision::Deny(why.to_string()));
        let name = p.tool_name.as_str();
        let target = path_of(&p.input);
        match name {
            "Read" | "Grep" | "Glob" | "LS" | "NotebookRead" => {
                let pattern = p.input["pattern"].as_str().unwrap_or("");
                let reaches_out = Path::new(pattern).is_absolute() || pattern.starts_with('~') || pattern.split('/').any(|c| c == "..");
                let path_key_ok = ["file_path", "path", "notebook_path"].iter().all(|k| p.input.get(*k).is_none_or(|v| v.is_string() || v.is_null()));
                let in_tree = match (wt, target) {
                    (Some(w), Some(t)) => inside(&w.path, t).is_some() && !reaches_out && path_key_ok,
                    (Some(_), None) => !reaches_out && path_key_ok, // a pattern search in the working directory
                    (None, _) => false,
                };
                if in_tree {
                    return Ok(Decision::Allow);
                }
                let payload = json!({"tool": name, "input": p.input});
                let summary = format!("{} {} (outside the task worktree)", name, summarize(name, &p.input));
                if let Some(d) = self.ask(sc, ApprovalTier::Amber, "harness_read", &format!("harness_read {}", summarize(name, &p.input)), &summary, &payload).await? {
                    return Ok(Decision::Deny(d.content));
                }
                // A file from outside the worktree has no provenance we can vouch for.
                if sc.span.observe_input(Tier::L0) {
                    sc.m.append(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": "harness read outside the worktree"}))).await?;
                }
                Ok(Decision::Allow)
            }
            "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
                let (Some(w), Some(t)) = (wt, target) else { return deny("there is no worktree to edit") };
                if inside(&w.path, t).is_none() {
                    return deny("edits are allowed only inside the task worktree, and never on a .git path");
                }
                match gate(sc.span.ceiling(), Action::WorktreeWrite) {
                    Gate::Deny => return deny("edits are not allowed from a span at this taint level"),
                    Gate::Red => {
                        let summary = format!("{} {} from a span tainted at {}", name, t, sc.span.ceiling());
                        if let Some(d) = self.ask(sc, ApprovalTier::Red, "harness_edit", "harness_edit", &summary, &json!({"tool": name, "path": t, "ceiling": sc.span.ceiling().as_str()})).await? {
                            return Ok(Decision::Deny(d.content));
                        }
                    }
                    Gate::Allow => {}
                }
                let c = Classified { tool: "harness_edit".into(), tier: ToolTier::Amber, kind: ActionKind::WorktreeEdit, summary: format!("{} {}", name, t), payload: json!({"tool": name, "path": t}), pattern: format!("harness_edit {}", name) };
                if let Need::Yes(tier) = self.broker.need(&sc.task_id, None, &c) {
                    if let Some(d) = self.ask(sc, tier, &c.tool, &c.pattern, &c.summary, &c.payload).await? {
                        return Ok(Decision::Deny(d.content));
                    }
                }
                Ok(Decision::Allow)
            }
            "Bash" => {
                let line = p.input["command"].as_str().unwrap_or("");
                if wt.is_none() {
                    return deny("there is no worktree to run commands in");
                }
                match gate(sc.span.ceiling(), Action::WorktreeWrite) {
                    Gate::Deny => return deny("commands are not allowed from a span at this taint level"),
                    Gate::Red => {
                        let summary = format!("run `{}` from a span tainted at {}", line.chars().take(120).collect::<String>(), sc.span.ceiling());
                        if let Some(d) = self.ask(sc, ApprovalTier::Red, "harness_bash", "harness_bash", &summary, &json!({"tool": "Bash", "command": line, "ceiling": sc.span.ceiling().as_str()})).await? {
                            return Ok(Decision::Deny(d.content));
                        }
                    }
                    Gate::Allow => {}
                }
                match self.exec.verdict_line_at(line, wt.as_ref().map(|w| w.path.as_path())) {
                    Verdict::Refused(why) => deny(&format!("refused by policy: {}", why)),
                    Verdict::Allowed(_) => Ok(Decision::Allow),
                    Verdict::Ask => {
                        let first = line.split_whitespace().next().unwrap_or("");
                        let c = Classified { tool: "harness_bash".into(), tier: ToolTier::Amber, kind: ActionKind::UnsandboxedExec, summary: format!("run `{}` in the worktree (not sandboxed)", line.chars().take(200).collect::<String>()), payload: json!({"tool": "Bash", "command": line, "cwd": "@worktree", "sandboxed": false}), pattern: format!("harness_bash {}", first) };
                        if let Need::Yes(tier) = self.broker.need(&sc.task_id, None, &c) {
                            if let Some(d) = self.ask(sc, tier, &c.tool, &c.pattern, &c.summary, &c.payload).await? {
                                return Ok(Decision::Deny(d.content));
                            }
                        }
                        Ok(Decision::Allow)
                    }
                }
            }
            "WebFetch" | "WebSearch" => match gate(sc.span.ceiling(), Action::Outbound) {
                Gate::Deny => deny("outbound requests are not allowed from a span at this taint level"),
                _ => {
                    let summary = format!("{} {}", name, summarize(name, &p.input));
                    match self.ask(sc, ApprovalTier::Red, "harness_outbound", "harness_outbound", &summary, &json!({"tool": name, "input": p.input})).await? {
                        Some(d) => Ok(Decision::Deny(d.content)),
                        None => Ok(Decision::Allow),
                    }
                }
            },
            "TodoWrite" | "TodoRead" => Ok(Decision::Allow),
            other => {
                let summary = format!("the Claude CLI wants to use `{}`", other);
                match self.ask(sc, ApprovalTier::Amber, "harness_tool", &format!("harness_tool {}", other), &summary, &json!({"tool": other, "input": p.input})).await? {
                    Some(d) => Ok(Decision::Deny(d.content)),
                    None => Ok(Decision::Allow),
                }
            }
        }
    }

    /// Run the whole span in the subscription CLI.
    pub(crate) async fn run_harness(&self, sc: &mut SpanCtx, ipr: &Ipr, ep: &Endpoint) -> Result<Result<String, HarnessErr>, EngineError> {
        let Some(cli) = ep.harness.as_deref().and_then(Cli::from_harness) else {
            return Ok(Err(HarnessErr::retry(format!("the `{}` harness is not implemented (only `claude` and `agy`)", ep.harness.as_deref().unwrap_or("?")))));
        };
        let Some(model) = self.cfg.get().resolve_model(ep, &sc.tier) else { return Ok(Err(HarnessErr::retry(format!("endpoint `{}` has no model for the `{}` tier", ep.id, sc.tier)))) };
        let wt: Option<Worktree> = self.exec.known_worktree(&sc.task_id);
        let builder = wt.is_some() && sc.manifest.execute.iter().any(|e| e == "shell-worktree-scoped");
        match cli {
            Cli::Claude => {
                if sc.span.ceiling() <= Tier::L1 {
                    return Ok(Err(HarnessErr::retry("launch refused: this span's inputs are untrusted (taint L1 or lower), and a harness runs its own loop")));
                }
            }
            Cli::Agy => {
                if builder {
                    return Ok(Err(HarnessErr::retry("the agy harness has no approval bridge (its headless mode cannot ask), so it cannot run a Builder")));
                }
                // It can read any file the operator can and its tools cannot be turned off, so nothing it
                // returns can be vouched for: the span is untrusted from the start.
                if sc.span.observe_input(Tier::L0) {
                    sc.m.append(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": "agy harness can read any file"}))).await?;
                }
            }
        }
        // Projects must come out of an agy run exactly as they went in.
        let project_state = |engine: &Engine| -> Vec<String> { engine.projects.iter().map(|(_, p)| engine.exec.worktrees.project_status(p)).collect() };
        let projects_before = (cli == Cli::Agy).then(|| project_state(self));
        // A role without a worktree gets an empty directory and no tools: it sees only its prompt.
        let (cwd, tools) = match (&wt, builder) {
            (Some(w), true) => (w.path.clone(), Tools::Standard),
            _ => {
                let d = self.exec.scratch_for(&sc.task_id).join("harness-empty");
                std::fs::create_dir_all(&d).map_err(|e| EngineError::Invalid(e.to_string()))?;
                (d, Tools::None)
            }
        };
        let program = if cli == Cli::Agy { self.settings.agy_program.clone() } else { self.settings.harness_program.clone() };
        let launch = Launch { cli, program, model, system_prompt: format!("{}\n\n{}", ipr.system_sections().join("\n\n"), harness_note(builder)), user_prompt: ipr.first_user_text(), cwd, tools, env: cli_env() };
        let guard = builder.then(|| GitGuard::snapshot(&wt.as_ref().expect("builder has a worktree").git_internals()));
        let before = builder.then(|| exec::snapshot(&wt.as_ref().expect("builder has a worktree").path));
        sc.m.append(sc.agent_ev("harness.started", json!({"endpoint_id": ep.id, "tools": if builder { "standard" } else { "none" }, "cap_tokens": self.settings.harness_token_cap}), ep, &launch.model)).await?;
        let mut session = match Session::start(&launch).await {
            Ok(s) => s,
            Err(e) => return Ok(Err(HarnessErr::retry(e))),
        };
        let (mut text_parts, mut final_text, mut result): (Vec<String>, Option<String>, Option<Event>) = (Vec::new(), None, None);
        let mut metered = 0u64;
        let mut failure: Option<(String, bool)> = None;
        let mut exhaustion: Option<delphi_adapter::exhaust::Exhaustion> = None;
        let mut used_tools = false;
        let (hub, span_id) = (self.hub.clone(), sc.span_id.clone());
        let mut live = LiveRedactor::new(sc.m.secrets());
        while let Some(ev) = session.next(self.settings.harness_idle).await {
            match ev {
                Event::Init { .. } => {}
                Event::RateLimit { ref status, resets_at, ref window, .. } => {
                    if let Some(x) = delphi_adapter::exhaust::classify_rate_limit_event(status, resets_at, window) {
                        exhaustion.get_or_insert(x);
                    }
                }
                Event::Text(t) => {
                    hub.push_delta(&span_id, DeltaKind::Text, &live.push(&t));
                    text_parts.push(t);
                }
                Event::ToolUse { name, input, .. } => {
                    used_tools = true;
                    let line = self.secrets.redact(&summarize(&name, &input));
                    sc.m.append(sc.agent_ev("harness.tool", json!({"tool": name, "summary": line}), ep, &launch.model)).await?;
                }
                Event::ToolResult { .. } => {
                    if let Some(g) = &guard {
                        let touched = g.restore();
                        if !touched.is_empty() {
                            sc.m.append(sc.ev("git.tamper_reverted", json!({"count": touched.len()}))).await?;
                        }
                    }
                }
                Event::Permission(p) => {
                    let d = self.harness_decide(sc, &wt, &p).await?;
                    // An approved action may already have run by the time anything fails, so it counts as acting.
                    if matches!(d, Decision::Allow) && !matches!(p.tool_name.as_str(), "Read" | "Grep" | "Glob" | "LS" | "NotebookRead") {
                        used_tools = true;
                    }
                    if session.decide(&p, &d).await.is_err() {
                        failure = Some(("the Claude CLI stopped listening before its permission request was answered".into(), false));
                        break;
                    }
                }
                Event::Usage(t) => {
                    metered += t.metered();
                    if metered > self.settings.harness_token_cap {
                        session.kill().await;
                        failure = Some((format!("the CLI used {} tokens, over the {} cap for a span; stopped", metered, self.settings.harness_token_cap), false));
                        break;
                    }
                }
                Event::Result { ref text, is_error, ref usage, .. } => {
                    if is_error {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                        if let Some(x) = delphi_adapter::exhaust::classify_cli_error(text, now) {
                            exhaustion.get_or_insert(x);
                        }
                        failure = Some((match &exhaustion {
                            Some(x) => format!("the CLI is out of quota: {}", self.secrets.redact(&x.to_string())),
                            None => format!("the CLI reported an error: {}", text.chars().take(200).collect::<String>()),
                        }, !used_tools));
                    }
                    metered = metered.max(usage.metered());
                    if metered > self.settings.harness_token_cap && failure.is_none() {
                        failure = Some((format!("the CLI used {} tokens, over the {} cap for a span", metered, self.settings.harness_token_cap), false));
                    }
                    final_text = Some(text.clone());
                    result = Some(ev.clone());
                    session.close_input();
                }
                Event::Exit { code } => {
                    if result.is_none() && failure.is_none() {
                        failure = Some((
                            match code {
                                Some(c) => format!("the {} CLI exited ({}) without a result; is it signed in? Run it once to log in", launch.program, c),
                                None => format!("the {} CLI stopped responding and was killed", launch.program),
                            },
                            !used_tools,
                        ));
                    }
                    break;
                }
            }
        }
        if result.is_none() && failure.is_none() {
            failure = Some((format!("the {} CLI stopped without a result", launch.program), !used_tools));
        }
        session.kill().await;
        let tail = live.finish();
        if !tail.is_empty() {
            hub.push_delta(&span_id, DeltaKind::Text, &tail);
        }
        if let Some(g) = &guard {
            let touched = g.restore();
            if !touched.is_empty() {
                sc.m.append(sc.ev("git.tamper_reverted", json!({"count": touched.len()}))).await?;
            }
        }

        // Usage is recorded whether or not the run succeeded.
        let (usage, cost) = match &result {
            Some(Event::Result { usage, total_cost_usd, .. }) => (*usage, *total_cost_usd),
            _ => (crate::harness::Tokens { output: metered, ..Default::default() }, 0.0),
        };
        let payload = UsagePayload {
            provider: if cli == Cli::Agy { "google" } else { "anthropic" }.into(),
            input_tokens: usage.input,
            cache_read_tokens: usage.cache_read,
            cache_write_tokens: usage.cache_write,
            output_tokens: usage.output,
            thinking_tokens: 0,
            cost_usd: 0.0,
            reserved_usd: 0.0,
            usage_source: if result.is_some() { "reported" } else { "estimated" }.into(),
            cost_model: "subscription".into(),
            outcome: if failure.is_some() { "error" } else { "ok" }.into(),
            reservation_id: None,
        };
        let mut batch = vec![sc.agent_ev("agent.usage", payload.to_value(), ep, &launch.model), sc.agent_ev("harness.cost", json!({"total_cost_usd_equivalent": cost, "metered_tokens": metered}), ep, &launch.model)];

        if let Some(x) = &exhaustion {
            batch.push(sc.ev("model.exhausted", json!({"endpoint_id": ep.id, "model_id": launch.model, "reason": x.reason, "reset_at": x.reset_at, "raw": self.secrets.redact(&x.raw)})));
            self.pool.record_exhausted(&ep.id, &launch.model, x);
        }

        // Files the run created or changed take this span's tier, as with any other execution.
        if let (Some(w), Some(before)) = (&wt, before) {
            let changed = exec::changed(&before, &exec::snapshot(&w.path));
            let tier = sc.span.output_tier(OutputCheck::Unverified);
            let mut idx = sc.m.index.lock().unwrap();
            for rel in changed.iter().take(500) {
                let Ok(bytes) = std::fs::read(w.path.join(rel)) else { continue };
                if bytes.len() > 8 << 20 {
                    continue;
                }
                let hash = blake3::hash(&bytes).to_hex().to_string();
                idx.stamp(Zone::Worktree, rel, &hash, tier);
                batch.push(sc.ev("provenance.stamp", delphi_core::provenance::Index::stamp_payload(Zone::Worktree, rel, &hash, tier)));
            }
        }
        if failure.is_none() {
            if let Some(before) = projects_before {
                if project_state(self) != before {
                    failure = Some(("the agy run changed a project checkout; its output is discarded (`git status` differs from before)".into(), false));
                }
            }
        }
        if let Some((reason, retryable)) = failure {
            sc.m.append_batch(batch).await?;
            return Ok(Err(HarnessErr { reason, retryable }));
        }
        let text = final_text.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| text_parts.join("\n"));
        let block = delphi_adapter::blocks::ContentBlock::Text { text: text.clone() };
        batch.push(sc.agent_ev("agent.block.complete", json!({"turn_index": 1, "role": "assistant", "block_index": 0, "block": serde_json::to_value(&block).expect("serializes")}), ep, &launch.model));
        batch.push(sc.agent_ev("agent.turn.complete", json!({"turn_index": 1, "stop_reason": "end_turn", "blocks": 1, "provider": if cli == Cli::Agy { "agy-cli" } else { "claude-cli" }}), ep, &launch.model));
        sc.m.append_batch(batch).await?;
        if self.pool.record_success(&ep.id, &launch.model) {
            sc.m.append(sc.ev("model.recovered", json!({"endpoint_id": ep.id, "model_id": launch.model}))).await?;
        }
        Ok(Ok(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::Answer;
    use crate::engine::Status;
    use crate::harness::tests::fake_cli;
    use crate::testkit::{events_of, setup_with_exec, Fx};
    use std::fs;
    use std::time::Duration;

    /// The testkit config with one subscription harness endpoint serving Claude.
    fn use_harness(f: &Fx) {
        use_harness_with(f, "[\"claude-sub\"]", "[\"claude-sub\"]")
    }

    fn use_harness_with(f: &Fx, order: &str, allowed: &str) {
        let text = crate::testkit::CONFIG
            .replace("[access]", "[[endpoints]]\nid = \"claude-sub\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"claude\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\n[access]")
            .replace("order = { claude = [\"direct\", \"backup\"] }", &format!("order = {{ claude = {} }}", order))
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", &format!("allowed_endpoints = {}", allowed));
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
    }

    fn fixture(cli_body: &str, tweak: impl FnOnce(&mut crate::engine::Settings)) -> (Fx, tempfile::TempDir) {
        fixture_with(cli_body, tweak, |_| {})
    }

    fn fixture_with(cli_body: &str, tweak: impl FnOnce(&mut crate::engine::Settings), exec: impl FnOnce(&mut crate::exec::ExecConfig)) -> (Fx, tempfile::TempDir) {
        let scripts = tempfile::tempdir().unwrap();
        let prog = fake_cli(scripts.path(), cli_body);
        let f = setup_with_exec(vec![], 5.0, |s| { s.harness_program = prog; tweak(s); }, |c| { c.sandbox = crate::exec::SandboxMode::Off; exec(c); });
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        use_harness(&f);
        (f, scripts)
    }

    const INIT: &str = r#"echo '{"type":"system","subtype":"init","session_id":"s1"}'"#;
    const FINISH: &str = r#"echo '{"type":"result","subtype":"success","result":"All done.","stop_reason":"end_turn","total_cost_usd":0.03,"usage":{"input_tokens":10,"cache_creation_input_tokens":200,"cache_read_input_tokens":5000,"output_tokens":40}}'"#;

    #[test]
    fn paths_must_stay_inside_the_worktree_and_away_from_dot_git() {
        let d = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.rs"), "x").unwrap();
        std::os::unix::fs::symlink("/etc", root.join("link")).unwrap();
        for ok in ["src/a.rs", "src/new.rs", "./src/../src/a.rs"] {
            assert!(inside(&root, ok).is_some(), "{}", ok);
        }
        assert!(inside(&root, &format!("{}/src/a.rs", root.display())).is_some());
        for bad in ["../x", "/etc/passwd", "src/../../x", ".git/hooks/pre-commit", "sub/.GIT/config", "link/passwd", "link", "link/../z", "link/../.git/hooks/x"] {
            assert!(inside(&root, bad).is_none(), "{}", bad);
        }
    }

    #[tokio::test]
    async fn a_non_builder_role_runs_in_the_cli_with_no_tools_and_is_metered_as_a_subscription() {
        let (f, _s) = fixture(&format!("{}\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"Plan: do X.\"}}],\"usage\":{{\"input_tokens\":10,\"output_tokens\":20}}}}}}'\n{}", INIT, FINISH), |_| {});
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "All done."), "{}", out.text);
        assert!(f.model.requests().is_empty(), "no API call is made on a harness endpoint");
        let started = events_of(&f, "harness.started");
        assert_eq!((started[0].payload["tools"].as_str(), started[0].endpoint_id.as_deref()), (Some("none"), Some("claude-sub")));
        let usage = events_of(&f, "agent.usage");
        assert_eq!((usage[0].payload["cost_model"].as_str(), usage[0].payload["usage_source"].as_str(), usage[0].payload["cost_usd"].as_f64()), (Some("subscription"), Some("reported"), Some(0.0)));
        assert_eq!((usage[0].payload["output_tokens"].as_u64(), usage[0].payload["cache_read_tokens"].as_u64()), (Some(40), Some(5000)));
        assert_eq!(events_of(&f, "harness.cost")[0].payload["total_cost_usd_equivalent"], 0.03);
        assert_eq!(f.m.budget.lock().unwrap().spent(), 0, "a subscription run spends no dollars");
        assert!(!events_of(&f, "agent.turn.complete").is_empty());
    }

    #[tokio::test]
    async fn a_builder_command_asks_the_operator_runs_and_its_files_are_stamped_and_git_internals_restored() {
        let body = format!(
            r#"{init}
echo '{{"type":"control_request","request_id":"r1","request":{{"subtype":"can_use_tool","tool_name":"Bash","input":{{"command":"touch made.txt"}},"tool_use_id":"tu1"}}}}'
read answer
case "$answer" in *'"behavior":"allow"'*)
  touch made.txt
  gd=$(sed 's/gitdir: //' .git)
  printf '#!/bin/sh\ntouch /tmp/never\n' > "$gd/../../hooks/pre-commit"
  chmod +x "$gd/../../hooks/pre-commit"
  echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"tu1","name":"Bash","input":{{"command":"touch made.txt"}}}}]}}}}'
  echo '{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"tu1","is_error":false,"content":""}}]}}}}';;
*) echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"was denied"}}]}}}}';;
esac
{finish}"#,
            init = INIT,
            finish = FINISH
        );
        let (f, _s) = fixture(&body, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await }) };
        let mut card = None;
        for _ in 0..400 {
            if let Some(c) = f.engine.broker.pending().into_iter().find(|c| c.tool == "harness_bash") {
                card = Some(c);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let card = card.expect("a card for the Bash request");
        assert!(card.summary.contains("touch made.txt") && card.summary.contains("not sandboxed"), "{}", card.summary);
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Allow, "console-1").unwrap();
        let out = task.await.unwrap().unwrap();
        assert_eq!(out.status, Status::Done, "{}", out.text);
        let wt = f.root.join("workspaces/w/worktrees").join(&out.task_id);
        assert!(wt.join("made.txt").exists(), "the approved command ran");
        assert!(!f.root.join("proj/.git/hooks/pre-commit").exists(), "the planted hook is removed");
        assert_eq!(events_of(&f, "git.tamper_reverted").len(), 1);
        assert!(events_of(&f, "provenance.stamp").iter().any(|e| e.payload["path"] == "made.txt"));
        assert_eq!(events_of(&f, "harness.tool")[0].payload["tool"], "Bash");
        assert_eq!(events_of(&f, "harness.started")[0].payload["tools"], "standard");
    }

    #[tokio::test]
    async fn permission_requests_outside_the_rules_are_denied_without_asking() {
        let req = |tool: &str, input: &str| format!(r#"echo '{{"type":"control_request","request_id":"r-{t}","request":{{"subtype":"can_use_tool","tool_name":"{t}","input":{i},"tool_use_id":"x"}}}}'
read a_{t}
echo "$a_{t}" >> answers.txt"#, t = tool, i = input);
        let body = format!("{init}\n{w}\n{g}\n{b}\n{e}\n{fin}", init = INIT,
            w = req("Write", r#"{"file_path":"/etc/evil"}"#),
            g = req("Write", r#"{"file_path":".git/hooks/pre-commit"}"#),
            b = req("Bash", r#"{"command":"git push origin main"}"#),
            e = req("Edit", r#"{"file_path":"../escape.txt"}"#),
            fin = FINISH);
        let (f, _s) = fixture_with(&body, |_| {}, |c| c.deny = vec!["git push*".into()]);
        let out = f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        assert_eq!(out.status, Status::Done, "{}", out.text);
        assert!(f.engine.broker.pending().is_empty() && events_of(&f, "approval.requested").is_empty(), "denials need no card");
        let wt = f.root.join("workspaces/w/worktrees").join(&out.task_id);
        let answers = fs::read_to_string(wt.join("answers.txt")).unwrap();
        assert_eq!(answers.matches("\"behavior\":\"deny\"").count(), 4, "{}", answers);
        assert!(answers.contains("only inside the task worktree") && answers.contains("refused by policy"), "{}", answers);
        assert!(!answers.contains("updatedPermissions"));
    }

    #[tokio::test]
    async fn a_run_over_its_token_cap_is_killed_and_escalated_with_its_usage_recorded() {
        let body = format!(
            "{init}\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"working\"}}],\"usage\":{{\"input_tokens\":900,\"output_tokens\":300}}}}}}'\nsleep 30",
            init = INIT
        );
        let (f, _s) = fixture(&body, |s| s.harness_token_cap = 1000);
        let t = std::time::Instant::now();
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("over the 1000 cap"), "{}", r),
            other => panic!("{:?}", other),
        }
        assert!(t.elapsed() < Duration::from_secs(10), "the CLI was killed, not waited for");
        let usage = events_of(&f, "agent.usage");
        assert_eq!((usage[0].payload["outcome"].as_str(), usage[0].payload["cost_model"].as_str()), (Some("error"), Some("subscription")));
    }

    #[tokio::test]
    async fn a_cli_that_exits_without_a_result_says_so_and_a_tainted_span_never_launches() {
        let (f, _s) = fixture("exit 3", |_| {});
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("without a result") && r.contains("signed in"), "{}", r),
            other => panic!("{:?}", other),
        }
        let (g, _s2) = fixture(&format!("{}\n{}", INIT, FINISH), |_| {});
        let out = g.engine.run_task(&g.m, "architect", "Review @project/app/vendor/x.js").await.unwrap();
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("launch refused"), "{}", r),
            other => panic!("{:?}", other),
        }
        assert!(events_of(&g, "harness.started").is_empty());
    }

    fn two_methods(f: &Fx) {
        use_harness_with(f, "[\"claude-sub\", \"direct\"]", "[\"direct\", \"claude-sub\"]");
    }

    #[tokio::test]
    async fn a_harness_that_cannot_start_work_falls_back_to_the_api_endpoint_and_says_so() {
        let scripts = tempfile::tempdir().unwrap();
        let prog = fake_cli(scripts.path(), "exit 3");
        let f = setup_with_exec(vec![crate::model::Step::text("Answered by the API.")], 5.0, |s| s.harness_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        two_methods(&f);
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Answered by the API."), "{}", out.text);
        let fb = events_of(&f, "harness.fallback");
        assert_eq!((fb[0].payload["from"].as_str(), fb[0].payload["to"].as_str()), (Some("claude-sub"), Some("api")));
        assert!(fb[0].payload["reason"].as_str().unwrap().contains("without a result"));
        assert_eq!(f.model.requests()[0].endpoint_id, "direct", "the harness endpoint is never used for an API request");
        // The failed harness attempt is still on the books, as an error.
        assert!(events_of(&f, "agent.usage").iter().any(|e| e.payload["cost_model"] == "subscription" && e.payload["outcome"] == "error"));
    }

    #[tokio::test]
    async fn a_harness_that_already_acted_is_never_retried_by_another_method() {
        let body = format!(
            "{init}\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{{\"command\":\"ls\"}}}}]}}}}'\nexit 2",
            init = INIT
        );
        let scripts = tempfile::tempdir().unwrap();
        let prog = fake_cli(scripts.path(), &body);
        let f = setup_with_exec(vec![crate::model::Step::text("must not be used")], 5.0, |s| s.harness_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        two_methods(&f);
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        assert!(matches!(out.status, Status::Escalated(_)), "{:?}", out.status);
        assert!(f.model.requests().is_empty() && events_of(&f, "harness.fallback").is_empty());
    }

    #[tokio::test]
    async fn when_every_api_endpoint_is_down_the_harness_takes_the_span() {
        let scripts = tempfile::tempdir().unwrap();
        let prog = fake_cli(scripts.path(), &format!("{}\n{}", INIT, FINISH));
        let f = setup_with_exec(vec![crate::model::Step::Throttle], 5.0, |s| s.harness_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        use_harness_with(&f, "[\"direct\", \"claude-sub\"]", "[\"direct\", \"claude-sub\"]");
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "All done."), "{}", out.text);
        let fb = events_of(&f, "harness.fallback");
        assert_eq!((fb[0].payload["from"].as_str(), fb[0].payload["to"].as_str()), (Some("api"), Some("claude-sub")));
        assert_eq!(events_of(&f, "endpoint.throttled").len(), 1);
        // A harness as capable as the target takes the span; the operator is not asked about a weaker API model first.
        assert!(events_of(&f, "model.degraded_confirm").is_empty() && events_of(&f, "task.blocked").is_empty());
    }

    /// A Gemini-family harness endpoint on `agy`, serving the `architect` and `builder` roles.
    fn use_agy(f: &Fx) {
        let text = crate::testkit::CONFIG
            .replace("[access]", "[[endpoints]]\nid = \"agy-sub\"\nfamily = \"gemini\"\npath = \"harness\"\nharness = \"agy\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\nmodels = { frontier = \"gemini-3.8-flash-low\", workhorse = \"gemini-3.8-flash-low\" }\n[access]\nrole_family = { architect = \"gemini\", builder = \"gemini\" }")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"backup\", \"agy-sub\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
    }

    #[tokio::test]
    async fn the_agy_harness_runs_read_only_roles_as_untrusted_and_never_a_builder() {
        let scripts = tempfile::tempdir().unwrap();
        let prog = fake_cli(
            scripts.path(),
            r#"echo '{"event":"init","init":{"model":"m","tools":[]}}'
echo '{"event":"step_update","step_update":{"step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"Findings: none."}}'
echo '{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"agent_response","usage":{"input_tokens":900,"output_tokens":12}}}'
echo '{"event":"result","result":{"status":"SUCCESS","response":"Findings: none.","usage":{"input_tokens":900,"output_tokens":12}}}'"#,
        );
        let f = setup_with_exec(vec![crate::model::Step::text("Built on the API.")], 5.0, |s| s.agy_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        use_agy(&f);
        let out = f.engine.run_task(&f.m, "architect", "Review the plan").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Findings: none."), "{}", out.text);
        assert!(events_of(&f, "span.taint").iter().any(|e| e.payload["ceiling"] == "L0" && e.payload["cause"].as_str().unwrap().contains("agy")), "its output is untrusted");
        let u = &events_of(&f, "agent.usage")[0].payload;
        assert_eq!((u["provider"].as_str(), u["cost_model"].as_str(), u["input_tokens"].as_u64()), (Some("google"), Some("subscription"), Some(900)));
        assert_eq!(events_of(&f, "harness.started")[0].payload["tools"], "none");
        // A Builder is never put on `agy` (it cannot ask the operator). Its usual family is Gemini and only `agy`
        // serves it, but the family is a preference: it is served by an eligible model instead (`specs/002` §4.1).
        crate::testkit::ratify_spec(&f);
        let b = f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        assert_eq!((b.status.clone(), b.text.as_str()), (Status::Done, "Built on the API."), "{}", b.text);
        assert_eq!(events_of(&f, "harness.started").len(), 1, "agy ran for the architect only");
        assert_eq!(f.model.requests().last().unwrap().endpoint_id, "direct", "the Builder went to an API endpoint");
    }

    /// A Gemini-family `agy` endpoint as the only allowed endpoint (no other family to fall back to).
    fn use_agy_only(f: &Fx) {
        let text = crate::testkit::CONFIG
            .replace("[access]", "[[endpoints]]\nid = \"agy-sub\"\nfamily = \"gemini\"\npath = \"harness\"\nharness = \"agy\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\nmodels = { frontier = \"gemini-3.8-flash-low\", workhorse = \"gemini-3.8-flash-low\" }\n[access]\nrole_family = { architect = \"gemini\" }")
            .replace("order = { claude = [\"direct\", \"backup\"] }", "order = { claude = [] }")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"agy-sub\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
    }

    const AGY_EXHAUSTED: &str = r#"echo '{"event":"init","init":{"model":"gemini-4-argon-medium","tools":[]}}'
echo '{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"error_message"}}'
echo '{"event":"result","result":{"status":"ERROR","response":"","error":"Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 136h12m54s.","duration_seconds":0,"num_turns":1,"usage":{"input_tokens":0,"output_tokens":0}}}'
exit 3"#;

    /// The Gemini subscription is out of quota for days; the span moves to an eligible Claude API endpoint.
    #[tokio::test]
    async fn an_exhausted_agy_span_moves_to_another_family_and_the_pool_remembers() {
        let scripts = tempfile::tempdir().unwrap();
        let prog = fake_cli(scripts.path(), AGY_EXHAUSTED);
        let f = setup_with_exec(vec![crate::model::Step::text("Answered by Claude.")], 5.0, |s| s.agy_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        use_agy(&f);
        let out = f.engine.run_task(&f.m, "architect", "Review the plan").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Answered by Claude."), "{}", out.text);
        let fb = events_of(&f, "harness.fallback");
        assert_eq!((fb[0].payload["from"].as_str(), fb[0].payload["to"].as_str()), (Some("agy-sub"), Some("api")));
        assert!(fb[0].payload["reason"].as_str().unwrap().contains("out of quota"));
        // The next task does not launch the dead subscription at all: the pool says it is out until the reset.
        f.model.respond_with(|_| crate::model::Step::text("Straight to the API."));
        let again = f.engine.run_task(&f.m, "architect", "And again").await.unwrap();
        assert_eq!(again.text, "Straight to the API.");
        assert_eq!(events_of(&f, "harness.started").len(), 1, "agy was launched once, not again while it is exhausted");
    }

    /// The output the live `agy` printed with the Gemini quota used up (`tests/fixtures/quota/`).
    /// The stream `claude` 2.1.310 printed with the five-hour window used up (`tests/fixtures/quota/`).
    #[tokio::test]
    async fn an_exhausted_claude_subscription_is_recorded_with_its_window_and_reset_and_escalates_with_the_reason() {
        // The two lines the CLI printed (quoted with a here-document: the text has an apostrophe).
        const RATE: &str = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1792864800,"rateLimitType":"five_hour","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":1,"resetsAt":1792864800,"status":"rejected","surpassedThreshold":1},"seven_day":{"utilization":0.62,"resetsAt":1793365200,"status":"allowed"}}},"uuid":"u","session_id":"s"}"#;
        const RESULT: &str = r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":429,"api_error":"usage_limit_reached","result":"You've hit your session limit · resets 2pm (America/New_York)","stop_reason":"stop_sequence","total_cost_usd":0,"usage":{"input_tokens":0,"output_tokens":0},"duration_ms":268}"#;
        let body = format!("{}\ncat <<'EOF'\n{}\n{}\nEOF\nexit 1", INIT, RATE, RESULT);
        let (f, _s) = fixture(&body, |_| {});
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("out of quota") && r.contains("five_hour") && r.contains("resets at"), "{}", r),
            other => panic!("{:?}", other),
        }
        let ex = events_of(&f, "model.exhausted");
        assert_eq!(ex.len(), 1);
        assert_eq!((ex[0].payload["endpoint_id"].as_str(), ex[0].payload["reset_at"].as_i64()), (Some("claude-sub"), Some(1_792_864_800)));
        assert!(ex[0].payload["reason"].as_str().unwrap().contains("five_hour"));
    }

    #[tokio::test]
    async fn an_exhausted_agy_model_says_why_and_when_it_returns_instead_of_an_empty_error() {
        let scripts = tempfile::tempdir().unwrap();
        let prog = fake_cli(
            scripts.path(),
            r#"echo '{"event":"init","init":{"model":"gemini-4-argon-medium","tools":[]}}'
echo '{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"error_message"}}'
echo '{"event":"result","result":{"status":"ERROR","response":"","error":"Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 136h12m54s.","duration_seconds":0,"num_turns":1,"usage":{"input_tokens":0,"output_tokens":0}}}'
exit 3"#,
        );
        let f = setup_with_exec(vec![], 5.0, |s| s.agy_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        use_agy_only(&f);
        let out = f.engine.run_task(&f.m, "architect", "Review the plan").await.unwrap();
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("out of quota") && r.contains("resets at"), "{}", r),
            other => panic!("{:?}", other),
        }
        let ex = events_of(&f, "model.exhausted");
        assert_eq!(ex.len(), 1, "one event for the one failed run");
        assert_eq!(ex[0].payload["endpoint_id"], "agy-sub");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let reset = ex[0].payload["reset_at"].as_i64().unwrap();
        assert!((reset - now - (136 * 3600 + 12 * 60 + 54)).abs() < 60, "reset about 5.7 days out: {}", reset - now);
        assert!(ex[0].payload["raw"].as_str().unwrap().contains("Individual quota reached"));
    }

    #[tokio::test]
    async fn an_agy_run_that_touches_a_project_checkout_is_discarded() {
        let scripts = tempfile::tempdir().unwrap();
        // The run starts in <root>/workspaces/w/scratch/<task>/harness-empty; the project is <root>/proj.
        let prog = fake_cli(
            scripts.path(),
            r#"echo '{"event":"init","init":{}}'
echo scribble > "$(pwd)/../../../../../proj/injected.txt"
echo '{"event":"result","result":{"status":"SUCCESS","response":"ok","usage":{}}}'"#,
        );
        let f = setup_with_exec(vec![], 5.0, |s| s.agy_program = prog, |c| c.sandbox = crate::exec::SandboxMode::Off);
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        use_agy(&f);
        let out = f.engine.run_task(&f.m, "architect", "Review the plan").await.unwrap();
        assert!(f.root.join("proj/injected.txt").exists(), "the stand-in did write into the project");
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("changed a project checkout"), "{}", r),
            other => panic!("{:?}", other),
        }
        assert!(!out.text.contains("ok"), "the output is discarded");
    }
}
