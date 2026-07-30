//! `orvena status` — a one-screen summary of the active config-first setup:
//! provider, tier, roles, gates, budgets, and discovered skills.

use super::{config_dir, is_initialized};
use anyhow::Result;
use orvena_core::config::Config;
use orvena_core::skills::SkillRegistry;

pub fn run() -> Result<()> {
    let dir = config_dir();
    if !is_initialized(&dir) {
        println!("Not initialized — run `orvena init`.");
        return Ok(());
    }
    let config = Config::load_dir(&dir)?;

    println!("Orvena status");
    println!("  provider:  {} (model {})", config.agent.provider.kind, config.agent.provider.model);
    println!("  tier:      {:?}", config.agent.tier);
    println!("  max_steps: {}", config.agent.max_steps);

    println!("  roles:");
    for r in &config.roles.roles {
        let marker = if r.name == config.agent.default_role { " (default)" } else { "" };
        println!(
            "    - {}{}: allow {:?} forbid {:?}",
            r.name, marker, r.allowed_tools, r.forbidden_tools
        );
    }

    println!("  gates:");
    for g in &config.gates.gates {
        println!("    - {} [{:?}]: {}", g.name, g.gatekeeper, g.condition);
    }

    println!("  budgets:   default {} tokens", config.budgets.default_max_tokens);
    for b in &config.budgets.budgets {
        println!("    - {}: {} tokens", b.role, b.max_tokens);
    }

    let registry = SkillRegistry::discover(dir.join("skills"))?;
    if registry.skills.is_empty() {
        println!("  skills:    (none)");
    } else {
        println!("  skills:");
        for s in &registry.skills {
            println!("    - {}: {}", s.name, s.description);
        }
    }

    Ok(())
}
