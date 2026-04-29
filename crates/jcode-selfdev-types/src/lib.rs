use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReloadRecoveryDirective {
    pub reconnect_notice: Option<String>,
    pub continuation_message: String,
}
