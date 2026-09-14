use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectInfo {
    pub project_id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: String,
    pub updated_at: String,
}
