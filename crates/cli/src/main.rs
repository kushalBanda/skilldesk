//! `skilldesk` command-line tool. Same core and database as the desktop app.

use clap::{Parser, Subcommand};
use skilldesk_core::actions::{ActionPlan, ActionRequest, FileOp};
use skilldesk_core::{scan, AgentKey, Core, Env, ItemKind, ItemStatus, KindFacts};
use std::path::PathBuf;
use std::process::ExitCode;

/// Exit code when a write was only shown: run again with `--yes`.
const NEEDS_YES: u8 = 2;

#[derive(Parser)]
#[command(name = "skilldesk", version, about = "Everything the coding agents on this Mac know and keep")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan agent files on this Mac and refresh the cache.
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Totals for the Dashboard (from the last scan).
    Overview {
        #[arg(long)]
        json: bool,
    },
    /// Changes made by SkillDesk, newest first.
    History {
        #[arg(long)]
        json: bool,
    },
    /// The Fix list (from the last scan), ranked.
    Fixes {
        #[arg(long)]
        json: bool,
    },
    /// Sessions of every agent (from the last scan), newest first; with a
    /// query, the sessions whose prompts match it.
    Sessions {
        query: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Secrets in agent history (from the last scan): masked, never the value.
    Secrets {
        /// Also list possible and ignored values.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Disk use by agent and the cleanup candidates. Measures again unless
    /// `--cached`. Moves nothing.
    Storage {
        #[arg(long)]
        cached: bool,
        #[arg(long)]
        json: bool,
    },
    /// List items of one kind (from the last scan), e.g. `skilldesk list skill`.
    List {
        kind: String,
        #[arg(long)]
        json: bool,
    },
    /// Agents SkillDesk knows: found on this Mac, read or switched off, skills folder.
    Agents {
        /// Also list agents not on this Mac.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Apply the one-click action of a Fix list card (id from `skilldesk fixes`).
    /// Shows the change; changes nothing without `--yes`.
    Fix {
        id: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Undo a change from History (id from `skilldesk history`). Shows the
    /// change; changes nothing without `--yes`.
    Undo {
        id: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Copy a skill into an agent's skills folder. Changes nothing without `--yes`.
    Link {
        skill: String,
        agent: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Remove a skill from an agent's skills folder (a folder goes to the
    /// Trash, a link is removed). Changes nothing without `--yes`.
    Unlink {
        skill: String,
        agent: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Copy a skill folder, `.zip` or `.skill` into each `--agent`'s skills
    /// folder. Changes nothing without `--yes`.
    Install {
        from: PathBuf,
        #[arg(long = "agent")]
        agents: Vec<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn describe(op: &FileOp) -> String {
    let d = |p: &std::path::Path| p.display().to_string();
    match op {
        FileOp::CreateSymlink { link, to, .. } => format!("link    {} -> {}", d(link), d(to)),
        FileOp::RemoveSymlink { link, .. } => format!("unlink  {}", d(link)),
        FileOp::UnlinkDangling { link, .. } => format!("unlink  {} (points nowhere)", d(link)),
        FileOp::RelinkDangling { link, .. } => format!("link    {}", d(link)),
        FileOp::MovePath { from, to } => format!("move    {} -> {}", d(from), d(to)),
        FileOp::EditText { path, .. } => format!("edit    {}", d(path)),
        FileOp::MaskText { path, .. } => format!("mask    {}", d(path)),
        FileOp::UnmaskText { path, .. } => format!("unmask  {}", d(path)),
    }
}

/// Shows the plan. With `--yes` applies it, rescans the config and prints the
/// History record; without, changes nothing and exits with `NEEDS_YES`.
fn plan_apply(core: &Core, req: ActionRequest, yes: bool, json: bool) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let plan: ActionPlan = core.action_plan(req)?.redacted();
    if !yes {
        core.action_discard(&plan.id);
        if json {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "applied": false, "plan": plan }))?);
        } else {
            println!("Plan, not applied: {}", plan.summary);
            for op in &plan.ops {
                println!("  {}", describe(op));
            }
            for r in &plan.removed {
                println!("  to the Trash: {}", r.path.display());
            }
            println!("Nothing changed. Run again with --yes to apply.");
        }
        return Ok(ExitCode::from(NEEDS_YES));
    }
    let record = core.action_apply(&plan.id)?.redacted();
    scan::rescan_config(core, &scan::NoSink)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "applied": true, "record": record }))?);
    } else {
        println!("{}", record.summary);
        println!("Done. Undo: skilldesk undo {} --yes", record.id);
    }
    Ok(ExitCode::SUCCESS)
}

fn agent_key(name: &str) -> Result<AgentKey, Box<dyn std::error::Error>> {
    let a = skilldesk_core::adapters::registry()
        .iter()
        .find(|a| a.key == name || a.display_name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("unknown agent: {name} (see skilldesk agents --all)"))?;
    Ok(a.agent_key())
}

fn skill_id(core: &Core, name: &str) -> Result<skilldesk_core::ItemId, Box<dyn std::error::Error>> {
    core.items(ItemKind::Skill)?
        .into_iter()
        .find(|i| i.name == name && !matches!(i.status, ItemStatus::Gone | ItemStatus::NotAnItem))
        .map(|i| i.id)
        .ok_or_else(|| format!("no skill named {name} (run skilldesk scan first?)").into())
}

fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let core = Core::open(Env::from_os())?;
    match cli.command {
        Command::Agents { all, json } => {
            let s = core.settings()?;
            let rows: Vec<_> = s.agents.into_iter().filter(|a| all || a.found).collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                for a in &rows {
                    let state = if !a.found { "not found" } else if a.on { "read" } else { "off" };
                    let tier = match a.tier {
                        skilldesk_core::AgentTier::Full => "full",
                        _ => "skills only",
                    };
                    println!("{:<20} {:<12} {:<10} {:>4} skills  {}", a.agent.0, tier, state, a.skills, a.skills_dir.display());
                }
            }
        }
        Command::Fix { id, yes, json } => {
            let fixes = core.fixes()?;
            let fix = fixes.iter().find(|f| f.id == id).ok_or_else(|| format!("no fix with id {id} (see skilldesk fixes)"))?;
            let req = fix.action.clone().ok_or("this fix has no one-click action; open it in SkillDesk")?;
            return plan_apply(&core, req, yes, json);
        }
        Command::Undo { id, yes, json } => {
            let rec = core.history()?.into_iter().find(|r| r.id == id).ok_or_else(|| format!("no change with id {id} (see skilldesk history)"))?;
            if !yes {
                if json {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "applied": false, "record": rec.redacted() }))?);
                } else {
                    println!("Plan, not applied: undo \"{}\"", rec.summary);
                    for op in rec.ops.iter().rev() {
                        println!("  {}", describe(&op.inverse()));
                    }
                    println!("Nothing changed. Run again with --yes to apply.");
                }
                return Ok(ExitCode::from(NEEDS_YES));
            }
            let rec = core.action_undo(&id)?.redacted();
            scan::rescan_config(&core, &scan::NoSink)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "applied": true, "record": rec }))?);
            } else {
                println!("Undone: {}", rec.summary);
            }
        }
        Command::Link { skill, agent, yes, json } => {
            let req = ActionRequest::SkillToggle { id: skill_id(&core, &skill)?, agent: agent_key(&agent)?, on: true };
            return plan_apply(&core, req, yes, json);
        }
        Command::Unlink { skill, agent, yes, json } => {
            let req = ActionRequest::SkillToggle { id: skill_id(&core, &skill)?, agent: agent_key(&agent)?, on: false };
            return plan_apply(&core, req, yes, json);
        }
        Command::Install { from, agents, yes, json } => {
            let from = std::fs::canonicalize(&from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
            let agents = agents.iter().map(|a| agent_key(a)).collect::<Result<Vec<_>, _>>()?;
            return plan_apply(&core, ActionRequest::SkillInstall { from, agents }, yes, json);
        }
        Command::Scan { json } => {
            let run_id = scan::run_scan(&core, &scan::NoSink)?;
            let o = core.overview()?;
            if json {
                println!("{}", serde_json::json!({ "run_id": run_id, "overview": o }));
            } else {
                for k in &o.kinds {
                    println!("{:<12} {}", k.kind.as_str(), k.count);
                }
            }
        }
        Command::Overview { json } => {
            let o = core.overview()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&o)?);
            } else {
                println!("Agents       {}", o.agents.len());
                if let (Some(p), Some(d)) = (o.prompts, o.active_days) {
                    println!("Prompts      {p} on {d} active days");
                }
                for r in &o.agent_rows {
                    let p: u64 = o.activity.iter().filter(|a| a.agent == r.agent).map(|a| a.prompts as u64).sum();
                    println!("  {:<14} {:>6} prompts {:>5} sessions {:>4} skills {:>4} hooks {:>4} MCP", r.name, p, r.sessions, r.skills, r.hooks, r.mcp);
                }
                for k in &o.kinds {
                    println!("{:<12} {}", k.kind.as_str(), k.count);
                }
                if let Some(h) = &o.hooks {
                    println!("Hooks        {} runs, {:.1} min, {} cancelled", h.runs, h.total_ms as f64 / 60000.0, h.cancelled);
                }
                println!("Scanned      {}", o.scanned_at.as_deref().unwrap_or("never"));
            }
        }
        Command::Fixes { json } => {
            let f = core.fixes()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&f)?);
            } else {
                for x in &f {
                    println!("[{:>3}] {}\n      {}\n      {} · {}  ({})", x.risk, x.title, x.why, x.action_label, x.link, x.id);
                }
            }
        }
        Command::History { json } => {
            let h: Vec<_> = core.history()?.iter().map(|r| r.redacted()).collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&h)?);
            } else {
                for r in &h {
                    println!("{}  {:<8} {}  ({})", r.at, format!("{:?}", r.status).to_lowercase(), r.summary, r.id);
                }
            }
        }
        Command::Sessions { query, json } => {
            let all = core.sessions()?;
            let hits = match query.as_deref() {
                Some(q) => Some(core.sessions_search(q)?),
                None => None,
            };
            let picked: Vec<(&skilldesk_core::sessions::Session, Option<String>)> = match &hits {
                Some(h) => h.iter().filter_map(|h| all.iter().find(|s| s.id == h.id).map(|s| (s, Some(h.snippet.replace(['\u{E000}', '\u{E001}'], "*"))))).collect(),
                None => all.iter().map(|s| (s, None)).collect(),
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&picked.iter().map(|(s, _)| s).collect::<Vec<_>>())?);
            } else {
                for (s, snip) in &picked {
                    let when = s.ended.or(s.started).and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
                    let proj = s.project.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    println!("{when}  {:<13} {:>4}  {:<20} {}", s.origin_name(), s.prompts, proj, s.title);
                    if let Some(x) = snip {
                        println!("            {x}");
                    }
                }
                println!("{} sessions", picked.len());
            }
        }
        Command::Secrets { all, json } => {
            use skilldesk_core::secrets::Confidence;
            let list: Vec<_> = core.secrets()?.into_iter().filter(|s| all || (s.confidence == Confidence::Likely && !s.ignored)).collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                for s in &list {
                    let when = s.last_seen.and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
                    let mut places: Vec<&str> = s.places.iter().map(|p| p.place.as_str()).collect();
                    places.sort_unstable();
                    places.dedup();
                    println!("{when}  {:<12} {:<24} {:>3} places  {}", s.kind.as_str(), s.masked, s.place_count, places.join(", "));
                }
                println!("{} secrets", list.len());
            }
        }
        Command::Storage { cached, json } => {
            let r = if cached { core.storage()? } else { core.storage_refresh()? };
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                let gb = |b: u64| format!("{:.2} GB", b as f64 / (1u64 << 30) as f64);
                for a in &r.agents {
                    let cats: Vec<String> = a.categories.iter().take(4).map(|c| format!("{:?} {}", c.category, gb(c.bytes))).collect();
                    println!("{:<16} {:>9}  {}", a.name, gb(a.bytes), cats.join(", "));
                }
                println!("total {} ({} ms)", gb(r.total_bytes), r.measure_ms);
                for c in &r.candidates {
                    println!("candidate {:<16} {:>9}  {} paths", c.kind.as_str(), gb(c.bytes), c.path_count);
                }
                println!("reclaimable {}; Trash holds {}", gb(r.reclaimable_bytes), r.trash_bytes.map(gb).unwrap_or_else(|| "?".into()));
            }
        }
        Command::List { kind, json } => {
            let kind = ItemKind::parse(&kind).ok_or_else(|| format!("unknown kind: {kind}"))?;
            let items = core.items(kind)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                for it in &items {
                    let detail = match &it.facts {
                        KindFacts::Skill(f) => format!("places {}", f.locations.len()),
                        KindFacts::Subagent(f) | KindFacts::Command(f) => format!(
                            "{:<10?} runs {:<6} places {}",
                            f.origin,
                            it.usage.as_ref().map(|u| u.count).unwrap_or(0),
                            f.locations.len()
                        ),
                        KindFacts::Memory(f) => format!(
                            "{:<10?} {:<22} {}{}",
                            f.mem_type,
                            f.project_label,
                            f.cluster.as_deref().map(|c| format!("same as {c} ")).unwrap_or_default(),
                            if f.stale.is_empty() { String::new() } else { format!("stale {:?}", f.stale) }
                        ),
                        KindFacts::Hook(h) => {
                            let t = it.usage.as_ref().and_then(|u| u.timing.as_ref());
                            format!(
                                "{:<12} {:<18} {:<14} runs {:<6} total {:>7.1}m p95 {}ms",
                                it.agents.first().map(|a| a.0.as_str()).unwrap_or(""),
                                h.event,
                                h.owner.as_deref().unwrap_or("-"),
                                it.usage.as_ref().map(|u| u.count).unwrap_or(0),
                                t.map(|t| t.total_ms as f64 / 60000.0).unwrap_or(0.0),
                                t.map(|t| t.p95_ms).unwrap_or(0),
                            )
                        }
                        KindFacts::Plugin(f) => format!(
                            "{:<40} uses {:<6} enabled {:?} skills {}",
                            f.id,
                            it.usage.as_ref().map(|u| u.count).unwrap_or(0),
                            f.enabled,
                            f.contents.skills
                        ),
                        KindFacts::Marketplace(f) => format!(
                            "{:<40} {:>6.1} MB plugins {:?}",
                            f.source,
                            f.size_bytes.unwrap_or(0) as f64 / 1_048_576.0,
                            f.plugins
                        ),
                        KindFacts::PluginCache(f) => format!("{:>6.1} MB", f.size_bytes as f64 / 1_048_576.0),
                        KindFacts::McpServer(f) => format!(
                            "calls {:<6} {} {}{}",
                            it.usage.as_ref().map(|u| u.count).unwrap_or(0),
                            if f.unconfigured { "(not in config)".to_string() } else {
                                f.locations.iter().map(|l| format!("{}{}", l.agent.0, if l.enabled { "" } else { "(off)" })).collect::<Vec<_>>().join(",")
                            },
                            if f.holds_keys { " [keys]" } else { "" },
                            ""
                        ),
                        KindFacts::None => String::new(),
                    };
                    println!("{:<40} {:<12} {}", it.name, it.status.as_str(), detail);
                }
                println!("{} items", items.len());
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
