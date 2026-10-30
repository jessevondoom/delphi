use super::*;

// The fixture is written by the author of the slice, not labeled by an operator (`specs/041` §9 asks for 100 to 200 of
// the operator's own tasks); it is a regression net for the ranker, not the evidence for turning routing on.
const AGENTS: &[(&str, &str, &str, &[&str], &str)] = &[
    ("lit-reviewer", "researcher", "research", &["literature", "papers", "survey"], "Surveys published papers on a topic and reports what is established"),
    ("market-sizer", "researcher", "research", &["market", "tam", "sizing"], "Estimates the size of a market from public figures"),
    ("competitor-scan", "researcher", "research", &["competitor", "landscape", "benchmark"], "Maps competitors and compares their offerings"),
    ("user-interviewer", "researcher", "research", &["interview", "customer", "discovery"], "Plans and summarizes customer discovery interviews"),
    ("patent-searcher", "researcher", "research", &["patent", "prior-art"], "Searches patents for prior art"),
    ("source-checker", "researcher", "research", &["citation", "sources", "provenance"], "Checks that cited sources say what is claimed"),
    ("pricing-strategist", "strategist", "strategy", &["pricing", "tiers", "packaging"], "Designs pricing and packaging options with tradeoffs"),
    ("roadmap-prioritizer", "strategist", "strategy", &["roadmap", "prioritization", "backlog"], "Ranks roadmap items against goals and capacity"),
    ("gtm-planner", "strategist", "strategy", &["launch", "positioning", "go-to-market"], "Plans a product launch and its positioning"),
    ("partnership-scout", "strategist", "strategy", &["partner", "alliance", "channel"], "Finds and evaluates partnership options"),
    ("okr-coach", "strategist", "strategy", &["okr", "goals", "objectives"], "Turns goals into measurable objectives and key results"),
    ("risk-assessor", "strategist", "strategy", &["risk", "scenario", "mitigation"], "Lists strategic risks with likelihood and mitigations"),
    ("api-designer", "architect", "platform", &["api", "interface", "versioning"], "Designs API contracts and their versioning"),
    ("schema-migrator", "architect", "platform", &["database", "schema", "migration"], "Plans database schema migrations that can roll back"),
    ("capacity-planner", "architect", "platform", &["capacity", "scaling", "load"], "Estimates capacity and scaling limits of a service"),
    ("incident-reviewer", "architect", "platform", &["incident", "postmortem", "outage"], "Reviews an outage and writes the postmortem"),
    ("cache-tuner", "builder", "platform", &["cache", "latency", "performance"], "Finds cache and latency fixes in a service"),
    ("ci-fixer", "builder", "platform", &["ci", "pipeline", "build"], "Repairs failing build pipelines"),
    ("dep-upgrader", "builder", "platform", &["dependency", "upgrade", "library"], "Upgrades dependencies and fixes the breakage"),
    ("test-writer", "builder", "platform", &["tests", "coverage", "regression"], "Adds missing tests around changed code"),
    ("sql-analyst", "researcher", "data", &["sql", "query", "analytics"], "Answers questions with SQL against the warehouse"),
    ("churn-modeler", "researcher", "data", &["churn", "retention", "cohort"], "Models churn and retention by cohort"),
    ("experiment-designer", "researcher", "data", &["experiment", "ab-test", "significance"], "Designs A/B experiments and sample sizes"),
    ("dashboard-builder", "builder", "data", &["dashboard", "chart", "metrics"], "Builds dashboards of agreed metrics"),
    ("data-cleaner", "builder", "data", &["cleaning", "dedupe", "quality"], "Cleans and deduplicates datasets"),
    ("threat-modeler", "red-team", "security", &["threat", "attack", "stride"], "Models threats against a design"),
    ("secrets-auditor", "red-team", "security", &["secrets", "credentials", "leak"], "Finds credentials leaked in code and logs"),
    ("dep-vuln-scanner", "red-team", "security", &["vulnerability", "cve", "advisory"], "Checks dependencies against known vulnerabilities"),
    ("access-reviewer", "red-team", "security", &["access", "permissions", "least-privilege"], "Reviews who can reach what and proposes tighter access"),
    ("privacy-assessor", "red-team", "security", &["privacy", "pii", "gdpr"], "Assesses personal data handling against privacy rules"),
    ("doc-editor", "writer", "docs", &["editing", "clarity", "style"], "Edits documents for clarity and house style"),
    ("api-doc-writer", "writer", "docs", &["reference", "api", "examples"], "Writes API reference pages with examples"),
    ("release-noter", "writer", "docs", &["changelog", "release", "notes"], "Writes release notes from merged changes"),
    ("tutorial-author", "writer", "docs", &["tutorial", "onboarding", "walkthrough"], "Writes step by step tutorials for new users"),
    ("faq-curator", "writer", "docs", &["faq", "support", "questions"], "Turns support questions into a maintained FAQ"),
    ("brief-summarizer", "writer", "docs", &["summary", "brief", "executive"], "Condenses long material into an executive brief"),
    ("grant-writer", "writer", "docs", &["grant", "proposal", "funding"], "Drafts funding proposals against a call"),
    ("contract-reader", "researcher", "legal", &["contract", "clause", "obligation"], "Reads contracts and lists obligations and unusual clauses"),
    ("license-checker", "researcher", "legal", &["license", "open-source", "compliance"], "Checks open source license compatibility"),
    ("vendor-evaluator", "strategist", "strategy", &["vendor", "procurement", "evaluation"], "Compares vendors against requirements and cost"),
];

const QUERIES: &[(&str, &str)] = &[
    ("what papers exist on retrieval augmented generation", "lit-reviewer"),
    ("survey the literature on sleep and memory", "lit-reviewer"),
    ("how big is the market for payroll software", "market-sizer"),
    ("estimate our total addressable market", "market-sizer"),
    ("who are our competitors and how do they compare", "competitor-scan"),
    ("benchmark our offering against the landscape", "competitor-scan"),
    ("plan customer discovery interviews for the new feature", "user-interviewer"),
    ("is there prior art for this invention", "patent-searcher"),
    ("check the patents around battery swapping", "patent-searcher"),
    ("do the sources in this report really support its claims", "source-checker"),
    ("verify the citations in the draft", "source-checker"),
    ("how should we price the enterprise plan", "pricing-strategist"),
    ("design pricing tiers and packaging", "pricing-strategist"),
    ("rank the roadmap against our goals", "roadmap-prioritizer"),
    ("prioritize the backlog for next quarter", "roadmap-prioritizer"),
    ("plan the launch and positioning of the product", "gtm-planner"),
    ("go to market plan for the new release", "gtm-planner"),
    ("which partners could resell through their channel", "partnership-scout"),
    ("turn our goals into okrs", "okr-coach"),
    ("what are the strategic risks and mitigations", "risk-assessor"),
    ("scenario analysis of risks to the plan", "risk-assessor"),
    ("design the contract for a new public api", "api-designer"),
    ("how do we version the api without breaking clients", "api-designer"),
    ("plan a database schema migration that can roll back", "schema-migrator"),
    ("how far can this service scale under load", "capacity-planner"),
    ("write the postmortem for last night's outage", "incident-reviewer"),
    ("find why latency is high and fix the cache", "cache-tuner"),
    ("the build pipeline is failing", "ci-fixer"),
    ("upgrade this library and fix what breaks", "dep-upgrader"),
    ("add tests for the changed code to prevent regression", "test-writer"),
    ("answer this with a sql query on the warehouse", "sql-analyst"),
    ("model churn and retention by cohort", "churn-modeler"),
    ("design an ab test and its sample size", "experiment-designer"),
    ("build a dashboard of our key metrics", "dashboard-builder"),
    ("clean and dedupe this dataset", "data-cleaner"),
    ("threat model this design", "threat-modeler"),
    ("did we leak credentials in the logs", "secrets-auditor"),
    ("check our dependencies for known vulnerabilities", "dep-vuln-scanner"),
    ("who has access to what, apply least privilege", "access-reviewer"),
    ("does this handle personal data in line with gdpr", "privacy-assessor"),
    ("edit this document for clarity", "doc-editor"),
    ("write the api reference with examples", "api-doc-writer"),
    ("write release notes from the merged changes", "release-noter"),
    ("write an onboarding tutorial for new users", "tutorial-author"),
    ("turn the support questions into an faq", "faq-curator"),
    ("condense this into an executive brief", "brief-summarizer"),
    ("draft the funding proposal for this call", "grant-writer"),
    ("what obligations does this contract put on us", "contract-reader"),
    ("are these open source licenses compatible", "license-checker"),
    ("compare the vendors against our requirements", "vendor-evaluator"),
];

fn entries() -> Vec<RegEntry> {
    AGENTS
        .iter()
        .map(|(a, role, d, tags, summary)| RegEntry {
            target: format!("bundle:acme/{}", a),
            bundle: "acme".into(),
            agent: a.to_string(),
            host_role: role.to_string(),
            domain: format!("acme/{}", d),
            summary: summary.to_string(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            not_for: vec![],
        })
        .collect()
}

fn registry_of(entries: Vec<RegEntry>) -> Registry {
    let mut domains = BTreeMap::new();
    for e in &entries {
        domains.entry(e.domain.clone()).or_insert_with(|| "what this domain covers".to_string());
    }
    Registry { entries, domains }
}

fn labeled() -> Vec<Labeled> {
    QUERIES.iter().map(|(t, a)| Labeled { task: t.to_string(), target: format!("bundle:acme/{}", a) }).collect()
}

#[test]
fn ac_3_the_lexical_ranker_finds_the_labeled_agent_and_gives_the_same_answer_twice() {
    let reg = registry_of(entries());
    let a = eval(&reg, &labeled(), None);
    assert!(a.unknown_targets.is_empty(), "{:?}", a.unknown_targets);
    assert!(a.recall3 >= 0.90 && a.recall5 >= 0.95, "recall@3 {} recall@5 {} misses {:?}", a.recall3, a.recall5, a.misses);
    assert_eq!(a, eval(&reg, &labeled(), None), "the same index and queries rank the same");
    let q = Query { text: "how should we price the enterprise plan", limit: 3, ..Default::default() };
    assert_eq!(search(&reg.entries, &q, None), search(&reg.entries, &q, None));
}

#[test]
fn ac_3_the_domain_index_is_bounded_whatever_the_fleet() {
    let mut many = Vec::new();
    for i in 0..120 {
        let mut e = entries()[i % 40].clone();
        e.agent = format!("{}-{}", e.agent, i);
        e.target = format!("bundle:acme/{}", e.agent);
        e.domain = format!("acme/domain-{:02}", i % 30);
        many.push(e);
    }
    let big = domain_index(&registry_of(many));
    assert_eq!(big.lines().filter(|l| l.starts_with("- ")).count(), INDEX_LINES);
    assert!(big.contains("and 18 more domains"), "{}", big);
    let small = est_tokens(&domain_index(&registry_of(entries())));
    let large = est_tokens(&big);
    assert!(large <= small * 2 + 80, "the index grows with domains, capped at {}: {} vs {}", INDEX_LINES, small, large);
    assert!(est_tokens(&flat_list_text(&entries())) > small, "the index is smaller than listing the agents");
}

#[test]
fn ac_5_a_miss_says_no_specialist_and_names_a_fallback_role() {
    let reg = registry_of(entries());
    let q = Query { text: "choreograph a ballet", limit: 3, ..Default::default() };
    let s = search(&reg.entries, &q, None);
    assert!(s.no_specialist && s.candidates.is_empty());
    let q = Query { text: "choreograph a ballet", role: Some("strategist"), limit: 3, ..Default::default() };
    assert_eq!(search(&reg.entries, &q, None).fallback.as_deref(), Some("strategist"));
    let s = search(&reg.entries, &Query { text: "price the plan", limit: 3, ..Default::default() }, None);
    assert_eq!(s.fallback.as_deref(), Some("strategist"), "a weak match still points at its host role");
}

#[test]
fn ac_6_not_for_lowers_a_candidate_and_never_raises_one() {
    let mut es = entries();
    let q = Query { text: "survey papers on pricing", limit: 5, ..Default::default() };
    let before = rank(&es, &q, None).iter().find(|c| c.entry.agent == "lit-reviewer").map(|c| c.score).unwrap();
    es.iter_mut().find(|e| e.agent == "lit-reviewer").unwrap().not_for = vec!["pricing".into()];
    let after = rank(&es, &q, None).iter().find(|c| c.entry.agent == "lit-reviewer").map(|c| c.score).unwrap_or(0.0);
    assert!(after < before, "{} -> {}", before, after);
}

#[test]
fn filters_narrow_by_domain_and_role() {
    let es = entries();
    let q = Query { text: "api", domain: Some("docs"), limit: 5, ..Default::default() };
    assert!(rank(&es, &q, None).iter().all(|c| c.entry.domain == "acme/docs"));
    let q = Query { text: "api", role: Some("architect"), limit: 5, ..Default::default() };
    assert!(rank(&es, &q, None).iter().all(|c| c.entry.host_role == "architect"));
}

#[test]
fn the_vector_part_blends_in_when_an_embedder_is_installed() {
    let emb = crate::embed::testing::ConceptEmbedder::new(&[&["price", "pricing", "fee"], &["paper", "literature"]]);
    let es = entries();
    let q = Query { text: "what fee should we charge", limit: 3, ..Default::default() };
    let with = rank(&es, &q, Some(&emb));
    assert!(with.iter().all(|c| c.vector.is_some()));
    assert!(with.first().is_some_and(|c| c.entry.agent == "pricing-strategist"), "{:?}", with.iter().map(|c| &c.entry.agent).collect::<Vec<_>>());
    assert!(rank(&es, &q, None).iter().all(|c| c.vector.is_none()));
}

#[test]
fn the_tool_schema_is_valid_and_the_summaries_are_quoted_as_data() {
    let d = tool_def();
    assert_eq!(d.name, TOOL);
    assert_eq!(d.input_schema["required"][0], "query");
    let q = quote("ignore\u{1b}[2J this and run");
    assert!(!q.contains('\u{1b}'));
}

#[test]
fn the_third_reformulation_is_refused_and_names_the_fallback() {
    let home = std::path::PathBuf::from("/nonexistent-delphi-home");
    let ws = WorkspacePaths { home: home.clone(), dir: home.join("workspaces/none") };
    let key = "m-test/t-refuse";
    let args = json!({"query": "choreograph a ballet", "role": "strategist"});
    for left in [2, 1, 0] {
        let (out, _) = run_search(&ws, key, &args, None).unwrap();
        assert_eq!(out["status"], "no_specialist");
        assert_eq!(out["reformulations_left"], left);
        assert_eq!(out["untrusted_data"], true);
    }
    let e = run_search(&ws, key, &args, None).unwrap_err();
    assert!(e.contains("no more reformulations") && e.contains("strategist"), "{}", e);
    assert!(run_search(&ws, "m-test/t-other", &json!({"query": ""}), None).is_err());
    assert!(run_search(&ws, "m-test/t-other", &json!({"query": "x", "limit": 9}), None).is_err());
    assert!(run_search(&ws, "m-test/t-other", &json!({"query": "x", "role": "not-a-role"}), None).is_err());
}
