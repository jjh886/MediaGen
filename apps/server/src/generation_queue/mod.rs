mod api;
mod callback;
mod credential;
mod records;
#[cfg(test)]
mod store_tests;
mod worker;

pub(crate) use api::{
    admin_list_queue, admin_queue_action, batch_task_details, enqueue_image, list_queue,
    queue_action, task_detail,
};
pub(crate) use credential::CredentialCipher;
pub(crate) use records::queued_image_records;
pub(crate) use worker::start_worker;

pub(crate) fn owner_key(token: &str) -> String {
    crate::access_control::hash_secret(token)
}
