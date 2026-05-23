use std::collections::HashMap;

pub struct AgentEnvInput<'a> {
    pub project_id: &'a str,
    pub work_execution_id: Option<&'a str>,
    pub agent_session_id: &'a str,
    pub worktree_path: Option<&'a str>,
    pub task_file: Option<&'a str>,
    pub cli_path: &'a str,
    pub canonical_repo: &'a str,
    pub role: &'a str,
    pub attach_mode: Option<&'a str>,
}

pub fn build_agent_envs(input: &AgentEnvInput<'_>) -> HashMap<String, String> {
    let mut map = HashMap::with_capacity(9);
    map.insert("CCX_PROJECT_ID".into(), input.project_id.into());
    if let Some(v) = input.work_execution_id {
        map.insert("CCX_WORK_EXECUTION_ID".into(), v.into());
    }
    map.insert("CCX_AGENT_SESSION_ID".into(), input.agent_session_id.into());
    if let Some(v) = input.worktree_path {
        map.insert("CCX_WORKTREE_PATH".into(), v.into());
    }
    if let Some(v) = input.task_file {
        map.insert("CCX_TASK_FILE".into(), v.into());
    }
    map.insert("CCX_CLI".into(), input.cli_path.into());
    map.insert("CCX_CANONICAL_REPO".into(), input.canonical_repo.into());
    map.insert("CCX_ROLE".into(), input.role.into());
    if let Some(v) = input.attach_mode {
        map.insert("CCX_ATTACH_MODE".into(), v.into());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_input<'a>() -> AgentEnvInput<'a> {
        AgentEnvInput {
            project_id: "01JTEST00000000000000000001",
            work_execution_id: Some("01JTEST00000000000000000002"),
            agent_session_id: "01JTEST00000000000000000003",
            worktree_path: Some("/repos/myproject/.ccx/we-001"),
            task_file: Some("/repos/myproject/.ccx/we-001/task.md"),
            cli_path: "/usr/local/bin/ccx",
            canonical_repo: "/repos/myproject",
            role: "worker",
            attach_mode: Some("new"),
        }
    }

    #[test]
    fn all_nine_vars_present_when_fully_populated() {
        let envs = build_agent_envs(&full_input());
        assert_eq!(envs.len(), 9);
        assert_eq!(envs["CCX_PROJECT_ID"], "01JTEST00000000000000000001");
        assert_eq!(envs["CCX_WORK_EXECUTION_ID"], "01JTEST00000000000000000002");
        assert_eq!(envs["CCX_AGENT_SESSION_ID"], "01JTEST00000000000000000003");
        assert_eq!(envs["CCX_WORKTREE_PATH"], "/repos/myproject/.ccx/we-001");
        assert_eq!(envs["CCX_TASK_FILE"], "/repos/myproject/.ccx/we-001/task.md");
        assert_eq!(envs["CCX_CLI"], "/usr/local/bin/ccx");
        assert_eq!(envs["CCX_CANONICAL_REPO"], "/repos/myproject");
        assert_eq!(envs["CCX_ROLE"], "worker");
        assert_eq!(envs["CCX_ATTACH_MODE"], "new");
    }

    #[test]
    fn optional_vars_absent_when_none() {
        let input = AgentEnvInput {
            project_id: "01JTEST00000000000000000001",
            work_execution_id: None,
            agent_session_id: "01JTEST00000000000000000003",
            worktree_path: None,
            task_file: None,
            cli_path: "/usr/local/bin/ccx",
            canonical_repo: "/repos/myproject",
            role: "controller",
            attach_mode: None,
        };
        let envs = build_agent_envs(&input);
        assert_eq!(envs.len(), 5);
        assert!(!envs.contains_key("CCX_WORK_EXECUTION_ID"));
        assert!(!envs.contains_key("CCX_WORKTREE_PATH"));
        assert!(!envs.contains_key("CCX_TASK_FILE"));
        assert!(!envs.contains_key("CCX_ATTACH_MODE"));
    }
}
