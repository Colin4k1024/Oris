#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub issue_id: String,
    pub title: String,
    pub allowed_paths: Vec<String>,
}

impl TaskSpec {
    pub fn new(
        issue_id: &str,
        title: &str,
        allowed_paths: Vec<String>,
    ) -> Result<Self, &'static str> {
        if allowed_paths.is_empty() {
            return Err("allowed_paths must not be empty");
        }

        Ok(Self {
            issue_id: issue_id.to_string(),
            title: title.to_string(),
            allowed_paths,
        })
    }
}
