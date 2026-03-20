use std::fmt;

use crate::storage::{RepairTask, SqliteRepository, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairWorkerConfig {
    pub max_retries: u32,
    pub batch_size: usize,
}

impl Default for RepairWorkerConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            batch_size: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairWorkerRunResult {
    pub scanned: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub deadlettered: usize,
    pub last_task_id: Option<i64>,
}

#[derive(Debug)]
pub enum RepairWorkerError {
    Storage(StorageError),
    InvalidConfig { message: String },
}

impl fmt::Display for RepairWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(source) => write!(f, "repair worker storage failed: {source}"),
            Self::InvalidConfig { message } => write!(f, "invalid repair worker config: {message}"),
        }
    }
}

impl std::error::Error for RepairWorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(source) => Some(source),
            Self::InvalidConfig { .. } => None,
        }
    }
}

impl From<StorageError> for RepairWorkerError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

pub trait RepairTaskExecutor {
    fn execute(&mut self, task: &RepairTask) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct NoopRepairTaskExecutor;

impl RepairTaskExecutor for NoopRepairTaskExecutor {
    fn execute(&mut self, task: &RepairTask) -> Result<(), String> {
        validate_repair_task(task)
    }
}

pub struct RepairWorker<'a, E>
where
    E: RepairTaskExecutor + ?Sized,
{
    repository: &'a mut SqliteRepository,
    executor: &'a mut E,
    config: RepairWorkerConfig,
}

impl<'a, E> RepairWorker<'a, E>
where
    E: RepairTaskExecutor + ?Sized,
{
    pub fn new(
        repository: &'a mut SqliteRepository,
        executor: &'a mut E,
        config: RepairWorkerConfig,
    ) -> Result<Self, RepairWorkerError> {
        if config.max_retries == 0 {
            return Err(RepairWorkerError::InvalidConfig {
                message: "max_retries must be greater than zero".to_owned(),
            });
        }
        if config.batch_size == 0 {
            return Err(RepairWorkerError::InvalidConfig {
                message: "batch_size must be greater than zero".to_owned(),
            });
        }

        Ok(Self {
            repository,
            executor,
            config,
        })
    }

    pub fn run_once(
        &mut self,
        namespace: &str,
    ) -> Result<RepairWorkerRunResult, RepairWorkerError> {
        let mut result = RepairWorkerRunResult {
            scanned: 0,
            succeeded: 0,
            failed: 0,
            deadlettered: 0,
            last_task_id: None,
        };

        for _ in 0..self.config.batch_size {
            let Some(task) = self
                .repository
                .claim_next_repair_task(namespace, self.config.max_retries)?
            else {
                break;
            };

            result.scanned += 1;
            result.last_task_id = Some(task.id);

            match self.executor.execute(&task) {
                Ok(()) => {
                    self.repository.succeed_repair_task(task.id)?;
                    result.succeeded += 1;
                }
                Err(message) => {
                    let deadletter = (task.retry_count + 1) >= i64::from(self.config.max_retries);
                    self.repository.fail_repair_task(task.id, &message, deadletter)?;
                    if deadletter {
                        result.deadlettered += 1;
                    } else {
                        result.failed += 1;
                    }
                }
            }
        }

        Ok(result)
    }
}

fn validate_repair_task(task: &RepairTask) -> Result<(), String> {
    match task.task_type.as_str() {
        "index_insert" => {
            validate_target_type(task, &["file"])?;
            validate_payload_keys(task, &["file_id", "chunk_ids", "model_id", "dimension", "error"])
        }
        "index_delete" => {
            validate_target_type(task, &["file", "chunk"])?;
            validate_payload_keys(task, &["chunk_ids", "error"])
        }
        other => Err(format!("unsupported repair task_type: {other}")),
    }
}

fn validate_target_type(task: &RepairTask, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&task.target_type.as_str()) {
        return Ok(());
    }

    Err(format!(
        "unsupported repair target_type for task_type {}: {}",
        task.task_type, task.target_type
    ))
}

fn validate_payload_keys(task: &RepairTask, required_keys: &[&str]) -> Result<(), String> {
    let payload = task
        .payload_json
        .as_deref()
        .ok_or_else(|| format!("repair task {} is missing payload_json", task.id))?;

    for key in required_keys {
        let quoted = format!("\"{key}\"");
        if !payload.contains(&quoted) {
            return Err(format!(
                "repair task {} payload_json is missing required key {key}",
                task.id
            ));
        }
    }

    Ok(())
}
