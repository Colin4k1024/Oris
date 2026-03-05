pub const EXPECTED_PROTOCOL_VERSION: &str = "0.1.0-experimental";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A2aSessionRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub task_id: String,
    pub task_summary: String,
}

impl A2aSessionRequest {
    pub fn start(
        sender_id: &str,
        protocol_version: &str,
        task_id: &str,
        task_summary: &str,
    ) -> Self {
        Self {
            sender_id: sender_id.to_string(),
            protocol_version: protocol_version.to_string(),
            task_id: task_id.to_string(),
            task_summary: task_summary.to_string(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol_version != EXPECTED_PROTOCOL_VERSION {
            return Err("incompatible a2a task session protocol version");
        }
        Ok(())
    }
}
