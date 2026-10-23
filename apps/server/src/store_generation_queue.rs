use std::cmp::Ordering;

use chrono::{DateTime, Utc};

use crate::store::{AppStore, GenerationJobRecord, GenerationJobState, TaskRecord};

impl AppStore {
    /// 同时写入任务与队列记录，避免只留下其中一半的不可执行数据。
    pub fn create_generation_job(
        &self,
        task: TaskRecord,
        mut job: GenerationJobRecord,
    ) -> Result<(), String> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let mut jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if tasks.iter().any(|item| item.id == task.id)
            || jobs.iter().any(|item| item.task_id == task.id)
        {
            return Err(format!("任务 {} 已存在", task.id));
        }
        job.queue_seq = jobs
            .iter()
            .filter(|item| item.task_kind == job.task_kind)
            .map(|item| item.queue_seq)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        tasks.push(task);
        jobs.push(job);
        drop(jobs);
        drop(tasks);
        self.save();
        Ok(())
    }

    pub fn active_generation_job_count(&self, kind: &str) -> usize {
        self.generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|job| job.task_kind == kind && job_is_counted(job))
            .count()
    }

    pub fn list_all_generation_jobs(
        &self,
        limit: usize,
        offset: usize,
    ) -> (Vec<GenerationJobRecord>, usize) {
        self.list_generation_jobs_for_kind(None, None, limit, offset)
    }

    pub fn list_generation_jobs(
        &self,
        owner_key: &str,
        limit: usize,
        offset: usize,
    ) -> (Vec<GenerationJobRecord>, usize) {
        self.list_generation_jobs_for_kind(Some(owner_key), None, limit, offset)
    }

    pub(crate) fn list_generation_jobs_for_kind(
        &self,
        owner_key: Option<&str>,
        kind: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> (Vec<GenerationJobRecord>, usize) {
        let jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut active = jobs
            .iter()
            .filter(|job| {
                job_is_visible(job)
                    && owner_key.is_none_or(|expected| job.owner_key == expected)
                    && kind.is_none_or(|kind| job.task_kind == kind)
            })
            .cloned()
            .collect::<Vec<_>>();
        active.sort_by(queue_order);
        let total = active.len();
        (active.into_iter().skip(offset).take(limit).collect(), total)
    }

    pub fn generation_job(
        &self,
        task_id: &str,
        owner_key: Option<&str>,
    ) -> Option<GenerationJobRecord> {
        self.generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|job| {
                job.task_id == task_id && owner_key.is_none_or(|expected| job.owner_key == expected)
            })
            .cloned()
    }

    pub fn claim_next_generation_job(
        &self,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Option<GenerationJobRecord> {
        self.claim_generation_job_matching(now, lease_until, |_| true)
    }

    pub(crate) fn claim_generation_job_matching(
        &self,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        eligible: impl Fn(&GenerationJobRecord) -> bool,
    ) -> Option<GenerationJobRecord> {
        let mut jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let index = jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| {
                job_is_visible(job)
                    && eligible(job)
                    && parse_time(&job.next_attempt_at).is_some_and(|ready_at| ready_at <= now)
            })
            .min_by(|(_, left), (_, right)| queue_order(left, right))
            .map(|(index, _)| index)?;
        let job = &mut jobs[index];
        job.state = GenerationJobState::Leased;
        job.lease_until = Some(lease_until.to_rfc3339());
        job.started_at.get_or_insert_with(|| now.to_rfc3339());
        job.updated_at = now.to_rfc3339();
        let claimed = job.clone();
        drop(jobs);
        self.save();
        Some(claimed)
    }

    pub fn recover_leased_generation_jobs(&self, now: DateTime<Utc>) -> usize {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let mut jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut recovered = 0;
        for job in jobs
            .iter_mut()
            .filter(|job| job.state == GenerationJobState::Leased)
        {
            if let Some(task) = tasks.iter().find(|task| {
                task.id == job.task_id && matches!(task.status.as_str(), "completed" | "cancelled")
            }) {
                job.state = if task.status == "completed" {
                    GenerationJobState::Completed
                } else {
                    GenerationJobState::Cancelled
                };
                job.lease_until = None;
                job.completed_at = task.completed_at.clone().or_else(|| Some(now.to_rfc3339()));
                job.updated_at = now.to_rfc3339();
                recovered += 1;
                continue;
            }
            job.state = GenerationJobState::RetryWait;
            job.lease_until = None;
            job.next_attempt_at = now.to_rfc3339();
            job.updated_at = now.to_rfc3339();
            if let Some(task) = tasks.iter_mut().find(|task| task.id == job.task_id) {
                task.status = "pending".to_string();
                task.completed_at = None;
            }
            recovered += 1;
        }
        drop(jobs);
        drop(tasks);
        if recovered > 0 {
            self.save();
        }
        recovered
    }

    pub fn requeue_generation_job(
        &self,
        task_id: &str,
        next_attempt_at: DateTime<Utc>,
        category: &str,
        error_summary: &str,
    ) {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let mut jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(job) = jobs
            .iter_mut()
            .find(|job| job.task_id == task_id && job.state == GenerationJobState::Leased)
        else {
            return;
        };
        job.state = GenerationJobState::RetryWait;
        job.retry_count = job.retry_count.saturating_add(1);
        job.next_attempt_at = next_attempt_at.to_rfc3339();
        job.last_failure_category = Some(category.to_string());
        job.last_error_summary = Some(error_summary.to_string());
        job.lease_until = None;
        job.updated_at = Utc::now().to_rfc3339();
        if let Some(task) = tasks.iter_mut().find(|task| task.id == task_id) {
            task.status = "pending".to_string();
            task.error_message = Some(error_summary.to_string());
            task.completed_at = None;
        }
        drop(jobs);
        drop(tasks);
        self.save();
    }

    pub fn finish_generation_job(
        &self,
        task_id: &str,
        state: GenerationJobState,
        error_summary: Option<&str>,
    ) {
        let mut jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(job) = jobs.iter_mut().find(|job| job.task_id == task_id) else {
            return;
        };
        // 取消可能与正在返回的插件结果并发发生，取消状态必须拥有最终优先级。
        if job.state == GenerationJobState::Cancelled && state != GenerationJobState::Cancelled {
            return;
        }
        let now = Utc::now().to_rfc3339();
        job.state = state;
        job.credential_ciphertext.clear();
        job.last_error_summary = error_summary.map(str::to_string);
        job.lease_until = None;
        job.updated_at = now.clone();
        job.completed_at = Some(now);
        drop(jobs);
        self.save();
    }

    pub fn apply_generation_queue_action(
        &self,
        owner_key: Option<&str>,
        action: &str,
        task_ids: &[String],
    ) -> Result<usize, String> {
        validate_action(action, task_ids)?;
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let mut jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut actionable = Vec::new();
        for task_id in task_ids {
            let job = jobs.iter().find(|job| {
                job.task_id == *task_id
                    && owner_key.is_none_or(|expected| job.owner_key == expected)
            });
            match job.map(|job| &job.state) {
                Some(GenerationJobState::Queued | GenerationJobState::RetryWait) => {
                    actionable.push(task_id.clone());
                }
                Some(GenerationJobState::Leased) if action == "cancel" => {
                    actionable.push(task_id.clone());
                }
                Some(GenerationJobState::Cancelled) if action == "cancel" => {}
                _ => return Err(format!("任务 {task_id} 已开始、已结束或不存在")),
            }
        }
        if action == "promote" {
            promote_jobs(&mut jobs, &actionable);
        } else {
            cancel_jobs(&mut tasks, &mut jobs, &actionable);
        }
        drop(jobs);
        drop(tasks);
        self.save();
        Ok(actionable.len())
    }

    pub fn cancel_task_or_generation_job(&self, task_id: &str) -> Result<TaskRecord, String> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let mut jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let task = tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or_else(|| "task not found".to_string())?;
        if let Some(job) = jobs.iter_mut().find(|job| job.task_id == task_id) {
            match job.state {
                GenerationJobState::Queued
                | GenerationJobState::RetryWait
                | GenerationJobState::Leased
                | GenerationJobState::Cancelled => cancel_job(task, job),
                _ => return Err("任务已经结束".to_string()),
            }
        } else {
            mark_task_cancelled(task);
        }
        let cancelled = task.clone();
        drop(jobs);
        drop(tasks);
        self.save();
        Ok(cancelled)
    }
}

fn job_is_visible(job: &GenerationJobRecord) -> bool {
    matches!(
        job.state,
        GenerationJobState::Queued | GenerationJobState::RetryWait
    )
}

fn job_is_counted(job: &GenerationJobRecord) -> bool {
    job_is_visible(job) || job.state == GenerationJobState::Leased
}

fn queue_order(left: &GenerationJobRecord, right: &GenerationJobRecord) -> Ordering {
    right
        .priority
        .cmp(&left.priority)
        .then_with(|| left.queue_seq.cmp(&right.queue_seq))
}

fn promote_jobs(jobs: &mut [GenerationJobRecord], task_ids: &[String]) {
    let mut first = std::collections::HashMap::<String, i64>::new();
    for job in jobs.iter().filter(|job| job_is_visible(job)) {
        first
            .entry(job.task_kind.clone())
            .and_modify(|seq| *seq = (*seq).min(job.queue_seq))
            .or_insert(job.queue_seq);
    }
    let now = Utc::now().to_rfc3339();
    for (index, task_id) in task_ids.iter().enumerate() {
        if let Some(job) = jobs.iter_mut().find(|job| job.task_id == *task_id) {
            job.priority = 200;
            job.queue_seq = first.get(&job.task_kind).copied().unwrap_or(0) - task_ids.len() as i64
                + index as i64;
            job.updated_at = now.clone();
        }
    }
}

fn cancel_jobs(tasks: &mut [TaskRecord], jobs: &mut [GenerationJobRecord], task_ids: &[String]) {
    for task_id in task_ids {
        if let (Some(task), Some(job)) = (
            tasks.iter_mut().find(|task| task.id == *task_id),
            jobs.iter_mut().find(|job| job.task_id == *task_id),
        ) {
            cancel_job(task, job);
        }
    }
}

fn cancel_job(task: &mut TaskRecord, job: &mut GenerationJobRecord) {
    let now = Utc::now().to_rfc3339();
    job.state = GenerationJobState::Cancelled;
    job.credential_ciphertext.clear();
    job.last_error_summary = Some("用户取消".to_string());
    job.lease_until = None;
    job.updated_at = now.clone();
    job.completed_at = Some(now);
    mark_task_cancelled(task);
}

fn mark_task_cancelled(task: &mut TaskRecord) {
    task.status = "cancelled".to_string();
    task.error_message = Some("用户取消".to_string());
    task.completed_at = Some(Utc::now().to_rfc3339());
}

fn validate_action(action: &str, task_ids: &[String]) -> Result<(), String> {
    if task_ids.is_empty() || task_ids.len() > 100 {
        return Err("任务数量必须在 1 到 100 之间".to_string());
    }
    if !matches!(action, "promote" | "cancel") {
        return Err("action 只支持 promote 或 cancel".to_string());
    }
    Ok(())
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}
