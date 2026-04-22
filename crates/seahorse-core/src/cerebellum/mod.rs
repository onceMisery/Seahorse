#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CerebellumConfig {
    pub max_pending_tasks: usize,
}

impl Default for CerebellumConfig {
    fn default() -> Self {
        Self {
            max_pending_tasks: 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTask {
    pub task_type: String,
    pub namespace: String,
}

impl ScheduledTask {
    pub fn new(task_type: &str, namespace: &str) -> Self {
        Self {
            task_type: task_type.to_owned(),
            namespace: namespace.to_owned(),
        }
    }
}

#[derive(Debug)]
pub struct Cerebellum {
    config: CerebellumConfig,
    pending_tasks: Vec<ScheduledTask>,
}

impl Cerebellum {
    pub fn new(config: CerebellumConfig) -> Self {
        Self {
            config,
            pending_tasks: Vec::new(),
        }
    }

    pub fn schedule(&mut self, task: ScheduledTask) {
        if self.pending_tasks.len() >= self.config.max_pending_tasks {
            return;
        }

        self.pending_tasks.push(task);
    }

    pub fn pending_tasks(&self) -> &[ScheduledTask] {
        &self.pending_tasks
    }
}
