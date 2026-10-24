//! Exact skill provenance, independent of the bank's current contents.
use super::LedgerEntry;
use crate::skill_bindings::ResolvedSkill;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkillVersion {
    #[serde(deserialize_with = "nonempty")]
    pub id: String,
    #[serde(deserialize_with = "nonempty")]
    pub version: String,
}

impl LedgerEntry {
    /// Capture the set resolved for the most recently admitted attempt.
    pub(crate) fn record_skills(&mut self, resolved: &[ResolvedSkill]) {
        let skills: Vec<_> = resolved
            .iter()
            .map(|skill| SkillVersion {
                id: skill.id.clone(),
                version: skill.version.clone(),
            })
            .collect();
        if let Some(route) = self.attempt_routing.last_mut() {
            route.skills.clone_from(&skills);
        }
        self.skills.extend(skills);
        self.skills.sort();
        self.skills.dedup();
    }

    /// Historical absence stays empty; never borrow another attempt's skills.
    pub fn skills_for_attempt(&self, attempt_number: u32) -> &[SkillVersion] {
        self.attempt_routing
            .iter()
            .find(|route| route.attempt_number == attempt_number)
            .map_or(&[], |route| route.skills.as_slice())
    }
}

pub(crate) fn group_key(skills: &[SkillVersion]) -> String {
    skills.first().map_or_else(
        || "unknown".into(),
        |skill| {
            format!(
                "{}@{}",
                skill.id.replace('%', "%25").replace('@', "%40"),
                skill.version.replace('%', "%25").replace('@', "%40")
            )
        },
    )
}

/// Project each entry onto one skill/version at a time. An attempt using two
/// skills contributes to both buckets; totals across buckets are not additive.
pub(crate) fn group_entries(
    entries: &[LedgerEntry],
) -> std::collections::BTreeMap<String, Vec<LedgerEntry>> {
    let mut groups = std::collections::BTreeMap::<String, Vec<LedgerEntry>>::new();
    for entry in entries {
        let mut skills = entry.skills.clone();
        skills.extend(
            entry
                .attempt_routing
                .iter()
                .flat_map(|route| route.skills.clone()),
        );
        skills.sort();
        skills.dedup();
        // Include the unknown bucket when any attempt has no provenance.
        let unknown = skills.is_empty()
            || entry
                .attempts
                .iter()
                .any(|a| entry.skills_for_attempt(a.attempt_number).is_empty());
        for selected in skills.into_iter().map(Some).chain(unknown.then_some(None)) {
            let mut projected = entry.clone();
            projected.skills = selected.clone().into_iter().collect();
            if !entry.attempts.is_empty() {
                projected.attempts.retain(|attempt| {
                    let bound = entry.skills_for_attempt(attempt.attempt_number);
                    selected
                        .as_ref()
                        .map_or_else(|| bound.is_empty(), |skill| bound.contains(skill))
                });
                if projected.attempts.is_empty() {
                    continue;
                }
                projected.usage = super::LedgerUsage::default();
                projected.attempts_started = entry
                    .attempts_started
                    .map(|_| projected.attempts.len() as u32);
                projected.attempts_completed = entry.attempts_completed.map(|_| {
                    projected
                        .attempts
                        .iter()
                        .filter(|a| a.exit_code.is_some())
                        .count() as u32
                });
                projected.validation_result = projected
                    .attempts
                    .last()
                    .and_then(|a| a.validation_result.clone());
                projected.failure_class = projected
                    .attempts
                    .last()
                    .and_then(|a| a.failure_class.clone());
                projected.backend_exit_code = projected.attempts.last().and_then(|a| a.exit_code);
                projected.duration_seconds =
                    projected.attempts.iter().map(|a| a.duration_seconds).sum();
            }
            groups
                .entry(group_key(&projected.skills))
                .or_default()
                .push(projected);
        }
    }
    groups
}

pub(crate) fn summarize(entries: &[LedgerEntry]) -> Vec<super::summary::GroupSummary> {
    group_entries(entries)
        .into_iter()
        .flat_map(|(key, entries)| {
            super::summary::build_grouped_summary(
                &entries,
                |_| key.clone(),
                |_| key.clone(),
                |_, _, _| key.clone(),
            )
            .unwrap_or_default()
        })
        .collect()
}

pub(super) fn display(groups: &[super::summary::GroupSummary]) {
    println!("\nGrouped by skill (attempts with multiple skills appear in each bucket):");
    for group in groups {
        println!(
            "  {}: {} entries, {} attempts, tokens={}, cost={}",
            group.group_key,
            group.entries,
            group.attempts,
            group
                .total_tokens
                .map_or_else(|| "unknown".into(), |tokens| tokens.to_string()),
            group
                .total_cost_usd
                .map_or_else(|| "unknown".into(), |cost| format!("${cost:.4}"))
        );
    }
}

fn nonempty<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "skill id/version must not be empty",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests;
