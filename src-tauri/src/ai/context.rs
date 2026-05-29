// Business-context storage. Six settings rows (migration 0010) hold
// the structured fields the request builder reads from. None of these
// values are credentials, so they live in SQLite alongside the other
// non-secret configuration.
//
// The UI displays a "this will be sent to your AI provider" warning
// next to any non-empty free-form field (industry, compliance) — the
// `flag_fields` helper below computes those flags so the warning copy
// and the request-preview render agree.

use rusqlite::{params, Connection, OptionalExtension};

use super::error::AiError;
use super::types::{
    BusinessContext, ContextFlags, EnvironmentType, Provider, RiskTolerance, TeamSize,
};
use crate::db::paths::app_data_dir;

const KEY_PROVIDER: &str = "ai_provider";
const KEY_INDUSTRY: &str = "ai_context_industry";
const KEY_JOB_ROLE: &str = "ai_context_job_role";
const KEY_ENV_TYPE: &str = "ai_context_environment_type";
const KEY_COMPLIANCE: &str = "ai_context_compliance";
const KEY_RISK: &str = "ai_context_risk_tolerance";
const KEY_TEAM: &str = "ai_context_team_size";

/// PR #69 — max characters the UI textarea allows + the storage
/// layer accepts for the job_role field. Kept loose enough for a
/// short paragraph; rejecting longer payloads stops a runaway
/// paste from blowing up the AI prompt.
pub const JOB_ROLE_MAX_LEN: usize = 500;

fn db_path() -> Result<std::path::PathBuf, AiError> {
    Ok(app_data_dir()
        .map_err(|e| AiError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, AiError> {
    Connection::open(db_path()?).map_err(AiError::from)
}

fn read_value(conn: &Connection, key: &str) -> Result<String, AiError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.unwrap_or_default())
}

fn write_value(key: &str, value: &str) -> Result<(), AiError> {
    let conn = open()?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                        updated_at = excluded.updated_at",
        params![key, value, now],
    )?;
    Ok(())
}

pub fn read_provider() -> Result<Option<Provider>, AiError> {
    let conn = open()?;
    Ok(Provider::from_storage(&read_value(&conn, KEY_PROVIDER)?))
}

pub fn write_provider(provider: Option<Provider>) -> Result<(), AiError> {
    let v = provider.map(|p| p.as_str()).unwrap_or("");
    write_value(KEY_PROVIDER, v)
}

pub fn read_context() -> Result<BusinessContext, AiError> {
    let conn = open()?;
    let industry = read_value(&conn, KEY_INDUSTRY)?;
    let job_role = read_value(&conn, KEY_JOB_ROLE)?;
    let env_raw = read_value(&conn, KEY_ENV_TYPE)?;
    let compliance_raw = read_value(&conn, KEY_COMPLIANCE)?;
    let risk_raw = read_value(&conn, KEY_RISK)?;
    let team_raw = read_value(&conn, KEY_TEAM)?;

    let compliance: Vec<String> = compliance_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(BusinessContext {
        industry,
        job_role,
        environment_type: EnvironmentType::from_storage(&env_raw),
        compliance,
        risk_tolerance: RiskTolerance::from_storage(&risk_raw),
        team_size: TeamSize::from_storage(&team_raw),
    })
}

pub fn write_context(ctx: &BusinessContext) -> Result<(), AiError> {
    let industry = ctx.industry.trim();
    if industry.len() > 120 {
        return Err(AiError::InvalidInput("industry"));
    }
    let job_role = ctx.job_role.trim();
    if job_role.len() > JOB_ROLE_MAX_LEN {
        return Err(AiError::InvalidInput("job_role"));
    }
    let compliance_joined = ctx
        .compliance
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    if compliance_joined.len() > 240 {
        return Err(AiError::InvalidInput("compliance"));
    }

    write_value(KEY_INDUSTRY, industry)?;
    write_value(KEY_JOB_ROLE, job_role)?;
    write_value(KEY_ENV_TYPE, ctx.environment_type.as_str())?;
    write_value(KEY_COMPLIANCE, &compliance_joined)?;
    write_value(KEY_RISK, ctx.risk_tolerance.as_str())?;
    write_value(KEY_TEAM, ctx.team_size.as_str())?;
    Ok(())
}

/// Compute the "this looks identifying" flags for a context value.
/// Industry is flagged whenever non-empty. A compliance entry is
/// flagged when it doesn't match a recognized framework token AND
/// contains a digit (likely a specific identifier rather than a
/// framework name).
pub fn flag_fields(ctx: &BusinessContext) -> ContextFlags {
    ContextFlags {
        industry_identifying: !ctx.industry.trim().is_empty(),
        compliance_identifying: ctx
            .compliance
            .iter()
            .any(|c| !is_known_framework(c) && c.chars().any(|ch| ch.is_ascii_digit())),
    }
}

fn is_known_framework(s: &str) -> bool {
    let upper = s.trim().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        // United States
        "PCI"
            | "PCI-DSS"
            | "PCIDSS"
            | "SOC2"
            | "SOC-2"
            | "HIPAA"
            | "HITRUST"
            | "NIST"
            | "NIST800-53"
            | "NIST-800-53"
            | "NIST-CSF"
            | "FEDRAMP"
            | "FED-RAMP"
            | "FISMA"
            | "CMMC"
            | "GLBA"
            | "FERPA"
            | "CCPA"
            | "CPRA"
            | "SOX"
            | "CIS"
            | "CSA"
            // Europe / international
            | "ISO27001"
            | "ISO-27001"
            | "ISO/IEC 27001"
            | "ISO27017"
            | "ISO27018"
            | "GDPR"
            | "DORA"
            | "NIS2"
            | "EBA-GL"
            | "BSI"
            | "C5"
            | "TISAX"
            | "ENISA"
            | "PSD2"
            | "EIDAS"
            // Asia / APAC
            | "PDPA"
            | "PDPB"
            | "PIPL"
            | "DPDP"
            | "DPDPA"
            | "APPI"
            | "MAS-TRM"
            | "RBI-GUIDELINES"
            | "RBI"
            | "ASIC"
            | "APRA"
            | "APRA-CPS-234"
            | "OAIC"
            | "PIPEDA"
            // generic / opt-out
            | "NONE"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn industry_identifying_is_true_for_any_non_empty_value() {
        let ctx = BusinessContext {
            industry: "fintech".into(),
            ..Default::default()
        };
        assert!(flag_fields(&ctx).industry_identifying);
        let empty = BusinessContext::default();
        assert!(!flag_fields(&empty).industry_identifying);
    }

    #[test]
    fn compliance_known_frameworks_are_not_flagged() {
        let ctx = BusinessContext {
            compliance: vec!["PCI".into(), "SOC2".into(), "HIPAA".into()],
            ..Default::default()
        };
        assert!(!flag_fields(&ctx).compliance_identifying);
    }

    #[test]
    fn compliance_identifier_like_values_are_flagged() {
        let ctx = BusinessContext {
            compliance: vec!["Acme-Order-9921".into()],
            ..Default::default()
        };
        assert!(flag_fields(&ctx).compliance_identifying);
    }
}
