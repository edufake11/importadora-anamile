//! Cola de trabajos en segundo plano (analizar proyectos, etc.).
//!
//! - Como mucho `MAX_RUNNING` trabajos a la vez; el resto espera en cola.
//! - Cada trabajo informa su progreso y puede cancelarse.
//! - Nunca bloquea el chat ni las animaciones: todo es asíncrono.

use serde::Serialize;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use tauri::async_runtime::JoinHandle;
use tokio::sync::Semaphore;

pub const MAX_RUNNING: usize = 2;
pub const MAX_ACTIVE: usize = 8;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub title: String,
    pub status: JobStatus,
    pub progress: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
}

pub enum JobEvent {
    Changed(Vec<JobInfo>),
    Finished(JobInfo),
}

type Emit = Arc<dyn Fn(JobEvent) + Send + Sync>;

struct Entry {
    info: JobInfo,
    handle: Option<JoinHandle<()>>,
}

pub struct Jobs {
    entries: Mutex<Vec<Entry>>,
    sem: Arc<Semaphore>,
    emit: Emit,
}

/// Permite a un trabajo informar su progreso.
#[derive(Clone)]
pub struct Progress {
    jobs: Arc<Jobs>,
    id: String,
}

impl Progress {
    pub fn set(&self, text: impl Into<String>) {
        let text = text.into();
        self.jobs.update(&self.id, |i| i.progress = Some(text));
    }
}

impl Jobs {
    pub fn new(emit: impl Fn(JobEvent) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(Vec::new()),
            sem: Arc::new(Semaphore::new(MAX_RUNNING)),
            emit: Arc::new(emit),
        })
    }

    pub fn list(&self) -> Vec<JobInfo> {
        self.entries.lock().unwrap().iter().map(|e| e.info.clone()).collect()
    }

    fn changed(&self) {
        (self.emit)(JobEvent::Changed(self.list()));
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut JobInfo)) {
        {
            let mut entries = self.entries.lock().unwrap();
            if let Some(e) = entries.iter_mut().find(|e| e.info.id == id) {
                f(&mut e.info);
            }
        }
        self.changed();
    }

    fn active(&self) -> usize {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e.info.status, JobStatus::Queued | JobStatus::Running))
            .count()
    }

    /// Encola un trabajo. Devuelve su id.
    pub fn spawn<F, Fut>(self: &Arc<Self>, title: &str, work: F) -> Result<String, String>
    where
        F: FnOnce(Progress) -> Fut + Send + 'static,
        Fut: Future<Output = Result<String, String>> + Send + 'static,
    {
        if self.active() >= MAX_ACTIVE {
            return Err(format!(
                "Ya hay {MAX_ACTIVE} trabajos en curso. Espera a que terminen o cancela alguno."
            ));
        }
        let id = format!("job-{}", NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed));
        let info = JobInfo {
            id: id.clone(),
            title: title.to_string(),
            status: JobStatus::Queued,
            progress: None,
            result: None,
            error: None,
            created_at: crate::db::now_str(),
        };
        {
            let mut entries = self.entries.lock().unwrap();
            entries.push(Entry { info, handle: None });
            // Conserva como mucho 20 trabajos terminados en la lista.
            while entries.len() > 20 {
                match entries
                    .iter()
                    .position(|e| !matches!(e.info.status, JobStatus::Queued | JobStatus::Running))
                {
                    Some(i) => {
                        entries.remove(i);
                    }
                    None => break,
                }
            }
        }
        let me = self.clone();
        let job_id = id.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let Ok(_permit) = me.sem.clone().acquire_owned().await else {
                return;
            };
            me.update(&job_id, |i| i.status = JobStatus::Running);
            let result = work(Progress {
                jobs: me.clone(),
                id: job_id.clone(),
            })
            .await;
            me.update(&job_id, |i| match result {
                Ok(r) => {
                    i.status = JobStatus::Done;
                    i.result = Some(r);
                }
                Err(e) => {
                    i.status = JobStatus::Failed;
                    i.error = Some(e);
                }
            });
            if let Some(info) = me.list().into_iter().find(|i| i.id == job_id) {
                (me.emit)(JobEvent::Finished(info));
            }
        });
        if let Some(e) = self.entries.lock().unwrap().iter_mut().find(|e| e.info.id == id) {
            e.handle = Some(handle);
        }
        self.changed();
        Ok(id)
    }

    pub fn cancel(&self, id: &str) -> bool {
        let found = {
            let mut entries = self.entries.lock().unwrap();
            match entries
                .iter_mut()
                .find(|e| e.info.id == id && matches!(e.info.status, JobStatus::Queued | JobStatus::Running))
            {
                Some(e) => {
                    if let Some(h) = e.handle.take() {
                        h.abort();
                    }
                    e.info.status = JobStatus::Cancelled;
                    true
                }
                None => false,
            }
        };
        if found {
            self.changed();
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn runs_with_limited_concurrency_and_reports() {
        let finished = Arc::new(Mutex::new(Vec::new()));
        let f2 = finished.clone();
        let jobs = Jobs::new(move |e| {
            if let JobEvent::Finished(i) = e {
                f2.lock().unwrap().push(i);
            }
        });
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        for n in 0..4 {
            let (r, p) = (running.clone(), peak.clone());
            jobs.spawn(&format!("t{n}"), move |prog| async move {
                let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(now, Ordering::SeqCst);
                prog.set("trabajando");
                tokio::time::sleep(Duration::from_millis(60)).await;
                r.fetch_sub(1, Ordering::SeqCst);
                if n == 3 {
                    Err("falló".to_string())
                } else {
                    Ok(format!("ok {n}"))
                }
            })
            .unwrap();
        }
        for _ in 0..100 {
            if finished.lock().unwrap().len() == 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(finished.lock().unwrap().len(), 4);
        assert!(peak.load(Ordering::SeqCst) <= MAX_RUNNING);
        let list = jobs.list();
        assert_eq!(list.iter().filter(|i| i.status == JobStatus::Done).count(), 3);
        assert_eq!(list.iter().filter(|i| i.status == JobStatus::Failed).count(), 1);
    }

    #[tokio::test]
    async fn can_cancel() {
        let jobs = Jobs::new(|_| {});
        let id = jobs
            .spawn("largo", |_| async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(String::new())
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(jobs.cancel(&id));
        assert_eq!(jobs.list()[0].status, JobStatus::Cancelled);
        assert!(!jobs.cancel(&id));
    }
}
