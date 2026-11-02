use anyhow::{anyhow, Context, Result};
use std::future::Future;
use tokio::{sync::watch, task::JoinSet};
use tracing::{error, info};

#[derive(Clone)]
pub struct Shutdown {
    receiver: watch::Receiver<bool>,
}

impl Shutdown {
    pub async fn cancelled(&mut self) {
        if *self.receiver.borrow() {
            return;
        }

        let _ = self.receiver.changed().await;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskExit {
    Finished,
    ShutdownRequested,
}

struct TaskCompletion {
    name: &'static str,
    exit: TaskExit,
}

pub struct TaskManager {
    shutdown: watch::Sender<bool>,
    tasks: JoinSet<Result<TaskCompletion>>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (shutdown, _) = watch::channel(false);
        Self {
            shutdown,
            tasks: JoinSet::new(),
        }
    }

    pub fn shutdown_handle(&self) -> Shutdown {
        Shutdown {
            receiver: self.shutdown.subscribe(),
        }
    }

    pub fn spawn<F>(&mut self, name: &'static str, task: F)
    where
        F: Future<Output = Result<TaskExit>> + Send + 'static,
    {
        self.tasks.spawn(async move {
            let exit = task.await.with_context(|| format!("task {name} failed"))?;
            Ok(TaskCompletion { name, exit })
        });
    }

    pub async fn run(mut self) -> Result<()> {
        let mut first_error = None;
        let mut shutdown_requested = false;

        while let Some(joined) = self.tasks.join_next().await {
            match joined {
                Ok(Ok(completion)) => match completion.exit {
                    TaskExit::ShutdownRequested => {
                        info!(task = completion.name, "shutdown requested");
                        shutdown_requested = true;
                        self.initiate_shutdown();
                    }
                    TaskExit::Finished => {
                        if shutdown_requested {
                            info!(task = completion.name, "task finished during shutdown");
                        } else {
                            first_error =
                                Some(anyhow!("task {} exited unexpectedly", completion.name));
                            shutdown_requested = true;
                            self.initiate_shutdown();
                        }
                    }
                },
                Ok(Err(err)) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                        shutdown_requested = true;
                        self.initiate_shutdown();
                    } else {
                        error!(error = ?err, "task failed during shutdown");
                    }
                }
                Err(err) => {
                    let err = anyhow!("task join failed: {err}");
                    if first_error.is_none() {
                        first_error = Some(err);
                        shutdown_requested = true;
                        self.initiate_shutdown();
                    } else {
                        error!(error = ?err, "task join failed during shutdown");
                    }
                }
            }
        }

        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn initiate_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{TaskExit, TaskManager};
    use anyhow::{anyhow, Result};
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn shutdown_request_stops_other_tasks() -> Result<()> {
        let mut manager = TaskManager::new();
        let mut worker_shutdown = manager.shutdown_handle();

        manager.spawn("worker", async move {
            worker_shutdown.cancelled().await;
            Ok(TaskExit::Finished)
        });
        manager.spawn("signal", async move {
            tokio::task::yield_now().await;
            Ok(TaskExit::ShutdownRequested)
        });

        manager.run().await?;
        Ok(())
    }

    #[tokio::test]
    async fn unexpected_task_exit_is_an_error() -> Result<()> {
        let mut manager = TaskManager::new();
        let mut worker_shutdown = manager.shutdown_handle();

        manager.spawn("worker", async move {
            worker_shutdown.cancelled().await;
            Ok(TaskExit::Finished)
        });
        manager.spawn("unexpected", async move { Ok(TaskExit::Finished) });

        let err = match manager.run().await {
            Ok(()) => return Err(anyhow!("unexpected task exit should fail")),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("task unexpected exited unexpectedly"));
        Ok(())
    }

    #[tokio::test]
    async fn task_errors_are_reported() -> Result<()> {
        let mut manager = TaskManager::new();
        manager.spawn("broken", async move { Err(anyhow!("boom")) });

        let err = match manager.run().await {
            Ok(()) => return Err(anyhow!("task error should bubble")),
            Err(err) => err,
        };
        let rendered = format!("{err:#}");
        assert!(rendered.contains("task broken failed"));
        assert!(rendered.contains("boom"));
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_handle_returns_immediately_after_shutdown() -> Result<()> {
        let manager = TaskManager::new();
        let mut shutdown = manager.shutdown_handle();

        manager.initiate_shutdown();
        timeout(Duration::from_millis(50), shutdown.cancelled()).await?;

        Ok(())
    }

    #[tokio::test]
    async fn panicking_tasks_are_reported_as_join_errors() -> Result<()> {
        let mut manager = TaskManager::new();
        manager.spawn("panic", async move {
            panic!("boom");
            #[allow(unreachable_code)]
            Ok(TaskExit::Finished)
        });

        let err = match manager.run().await {
            Ok(()) => return Err(anyhow!("panic should fail the manager")),
            Err(err) => err,
        };
        assert!(err.to_string().contains("task join failed"));
        Ok(())
    }
}
