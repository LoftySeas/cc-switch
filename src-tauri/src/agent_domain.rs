//! Agent OS 核心领域类型。
//!
//! 本模块只定义稳定 Agent 身份与生命周期。Role、Capability、Permission、
//! Runtime、Provider 和 Model 均属于独立边界，不在 Agent 记录中内嵌。

use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycle {
    Draft,
    Active,
    Suspended,
    Retired,
}

impl AgentLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Retired => "retired",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "retired" => Ok(Self::Retired),
            other => Err(AppError::Database(format!(
                "Unknown agent lifecycle state in database: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub owner: String,
    pub lifecycle_state: AgentLifecycle,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub owner: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub owner: Option<String>,
}
