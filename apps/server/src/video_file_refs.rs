use std::path::Path;

use crate::file_api::read_file_as_data_url;
use crate::image_request::OpenAiImageRef;
use crate::media_execution::ApiError;
use crate::video_request::OpenAiVideoRequest;

pub(crate) async fn resolve_openai_video_file_refs(
    file_root: &Path,
    request: &mut OpenAiVideoRequest,
    owner_hash: &str,
) -> Result<(), ApiError> {
    if let Some(reference) = &mut request.input_reference {
        resolve_ref(file_root, reference, owner_hash).await?;
    }
    for reference in &mut request.input_references {
        resolve_ref(file_root, reference, owner_hash).await?;
    }
    if let Some(reference) = request.extra.get_mut("video_reference") {
        if let Some(id) = reference["file_id"].as_str() {
            let record = crate::file_api::read_record_for_owner(file_root, id, owner_hash)
                .await
                .map_err(|message| (400, "invalid_file_id", message))?;
            let data = read_file_as_data_url(file_root, id, owner_hash)
                .await
                .map_err(|message| (400, "invalid_file_id", message))?;
            *reference = serde_json::json!({"type":"input_file","filename":record.filename,"file_data":data});
        }
    }
    Ok(())
}

async fn resolve_ref(
    file_root: &Path,
    reference: &mut OpenAiImageRef,
    owner_hash: &str,
) -> Result<(), ApiError> {
    if reference
        .image_url
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    let Some(file_id) = reference
        .file_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    reference.image_url = Some(
        read_file_as_data_url(file_root, file_id, owner_hash)
            .await
            .map_err(|message| (400, "invalid_file_id", message))?,
    );
    Ok(())
}
