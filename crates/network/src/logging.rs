/// Logs that a task has started using `log::debug!`.
pub fn log_task_started(task_name: &str) {
    log::debug!("Started task '{task_name}'");
}

/// Logs that a task has stopped using `log::debug!`.
pub fn log_task_stopped(task_name: &str) {
    log::debug!("Stopped task '{task_name}'");
}

/// Logs that a task was aborted using `log::debug!`.
pub fn log_task_aborted(task_name: &str) {
    log::debug!("Aborted task '{task_name}'");
}
