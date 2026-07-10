use std::path::Path;

use crate::file_api::read_file_as_data_url;
use crate::image_request::{OpenAiImageRef, OpenAiImageRequest};

pub(crate) async fn resolve_openai_file_refs(
    file_root: &Path,
    request: &mut OpenAiImageRequest,
    owner_hash: &str,
) -> Result<(), (u16, &'static str, String)> {
    for image in &mut request.images {
        resolve_ref(file_root, image, owner_hash).await?;
    }
    if let Some(mask) = &mut request.mask {
        resolve_ref(file_root, mask, owner_hash).await?;
    }
    Ok(())
}

async fn resolve_ref(
    file_root: &Path,
    item: &mut OpenAiImageRef,
    owner_hash: &str,
) -> Result<(), (u16, &'static str, String)> {
    if item
        .image_url
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return Ok(());
    }
    let Some(file_id) = item.file_id.as_deref().filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    item.image_url = Some(
        read_file_as_data_url(file_root, file_id, owner_hash)
            .await
            .map_err(|message| (400, "invalid_file_id", message))?,
    );
    Ok(())
}
