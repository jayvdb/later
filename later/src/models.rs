use crate::{JobId, UtcDateTime};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "ty")]
pub(crate) enum MqMessage {
    PollDelayedJobs,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct Job {
    pub id: JobId,

    pub payload_type: String,
    pub payload: Vec<u8>,

    pub config: JobConfig,
    pub stage: Stage,
    pub previous_stages: Vec<Stage>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct JobConfig {
    pub total_retries: usize,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self { total_retries: 6 }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
pub(crate) enum Stage {
    /// Scheduled for later or waiting for
    Delayed(DelayedStage),
    Waiting(WaitingStage),
    Enqueued(EnqueuedStage),
    Running(RunningStage),
    Requeued(RequeuedStage),
    Success(SuccessStage),
    Failed(FailedStage),
}

pub trait StageName {
    fn get_name() -> String;
}

impl StageName for DelayedStage {
    fn get_name() -> String {
        "delayed".into()
    }
}

impl StageName for WaitingStage {
    fn get_name() -> String {
        "waiting".into()
    }
}

impl StageName for RequeuedStage {
    fn get_name() -> String {
        "requeued".into()
    }
}

impl DelayedStage {
    pub fn is_time(&self) -> bool {
        chrono::Utc::now() >= self.not_before
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct DelayedStage {
    pub date: UtcDateTime,

    pub not_before: UtcDateTime,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct WaitingStage {
    pub date: UtcDateTime,

    pub parent_id: JobId,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct EnqueuedStage {
    pub date: UtcDateTime,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct RunningStage {
    pub date: UtcDateTime,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct SuccessStage {
    pub date: UtcDateTime,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct FailedStage {
    pub date: UtcDateTime,
    pub reason: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct RequeuedStage {
    pub date: UtcDateTime,
    pub requeue_count: usize,
}

impl Job {
    pub fn transition(self) -> Job {
        let next_stage = self.stage.clone().transition();
        println!(
            "Transition job {}: {} -> {}",
            self.id,
            self.stage.get_name(),
            next_stage.get_name()
        );
        self.transition_to(next_stage)
    }

    fn transition_to(self, next_stage: Stage) -> Job {
        let last_stage = self.stage.clone();
        let mut job = Job {
            stage: next_stage,
            ..self
        };
        job.previous_stages.push(last_stage);
        job
    }

    pub fn transition_req(self) -> anyhow::Result<Job> {
        let req_count = self.previous_stages.iter().filter(|s| s.is_req()).count() + 1;
        self.transition_to_terminal_stage(Stage::Requeued(RequeuedStage {
            date: chrono::Utc::now(),
            requeue_count: req_count,
        }))
    }

    pub fn transition_success(self) -> anyhow::Result<Job> {
        self.transition_to_terminal_stage(Stage::Success(SuccessStage {
            date: chrono::Utc::now(),
        }))
    }

    #[allow(dead_code)]
    pub fn transition_failed(self, reason: String) -> anyhow::Result<Job> {
        self.transition_to_terminal_stage(Stage::Failed(FailedStage {
            date: chrono::Utc::now(),
            reason,
        }))
    }

    fn transition_to_terminal_stage(self, next_stage: Stage) -> anyhow::Result<Job> {
        if self.stage.is_terminal() {
            return Err(anyhow::anyhow!(
                "Can not transition as job is already is at terminal stage."
            ));
        }
        if let Stage::Running(_) = self.stage {
            return Ok(self.transition_to(next_stage));
        }
        return Err(anyhow::anyhow!(
            "Job is not in correct stage to transition to terminal state"
        ));
    }
}

impl Stage {
    pub fn get_name(&self) -> String {
        match self {
            Stage::Delayed(_) => DelayedStage::get_name(),
            Stage::Waiting(_) => WaitingStage::get_name(),
            Stage::Enqueued(_) => "enqueued".into(),
            Stage::Running(_) => "running".into(),
            Stage::Requeued(_) => RequeuedStage::get_name(),
            Stage::Success(_) => "success".into(),
            Stage::Failed(_) => "failed".into(),
        }
    }

    /// ## Before running
    /// * Delayed -> Scheduled for later
    /// * Waiting -> Waiting for parent job to complete
    ///
    /// ## Running
    /// * Enqueued -> Published
    /// * Running -> A worker accepted the job and running
    ///
    /// ## After running for at least once
    /// * Requeued -> Job failed and retried ... (Next: Enqueued)
    /// * Success -> Job is successful
    pub fn transition(self) -> Stage {
        match self {
            Stage::Delayed(_) => Stage::Enqueued(EnqueuedStage {
                date: chrono::Utc::now(),
            }),
            Stage::Waiting(_) => Stage::Enqueued(EnqueuedStage {
                date: chrono::Utc::now(),
            }),
            Stage::Enqueued(_) => Stage::Running(RunningStage {
                date: chrono::Utc::now(),
            }),
            Stage::Running(_) => todo!(),
            Stage::Requeued(_) => Stage::Enqueued(EnqueuedStage {
                date: chrono::Utc::now(),
            }),
            Stage::Success(_) => self, /* Terminal */
            Stage::Failed(_) => self,  /* Terminal */
        }
    }

    /// Some job requires polling in order to determine if they are
    /// eligible to start (eg. delayed job, requed etc.)
    pub fn is_polling_required(&self) -> bool {
        match self {
            Stage::Delayed(_) | Stage::Requeued(_) => true,
            _ => false,
        }
    }

    pub fn is_req(&self) -> bool {
        match self {
            Stage::Requeued(_) => true,
            _ => false,
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            Stage::Success(_) | Stage::Failed(_) => true,

            Stage::Delayed(_)
            | Stage::Waiting(_)
            | Stage::Enqueued(_)
            | Stage::Running(_)
            | Stage::Requeued(_) => false,
        }
    }

    pub fn is_success(&self) -> bool {
        match self {
            Stage::Success(_) => true,

            _ => false,
        }
    }
}
