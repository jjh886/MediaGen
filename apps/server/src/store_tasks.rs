use crate::store::AppStore;

impl AppStore {
    pub fn delete_task(&self, id: &str) -> bool {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let before = tasks.len();
        tasks.retain(|task| task.id != id);
        let changed = tasks.len() != before;
        drop(tasks);
        if changed {
            self.save();
        }
        changed
    }
}
