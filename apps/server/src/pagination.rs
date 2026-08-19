use serde::Deserialize;

pub(crate) const DEFAULT_PAGE_SIZE: usize = 40;
const MAX_PAGE_SIZE: usize = 100;

/// 所有管理列表共用同一套分页边界，避免某个接口意外返回全部历史数据。
#[derive(Debug, Default, Deserialize)]
pub(crate) struct PageQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

impl PageQuery {
    pub(crate) fn new(limit: Option<usize>, offset: Option<usize>) -> Self {
        Self { limit, offset }
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    pub(crate) fn offset(&self) -> usize {
        self.offset.unwrap_or(0)
    }
}

pub(crate) fn paginate<T>(items: Vec<T>, query: &PageQuery) -> Vec<T> {
    items
        .into_iter()
        .skip(query.offset())
        .take(query.limit())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{paginate, PageQuery, DEFAULT_PAGE_SIZE};

    #[test]
    fn defaults_to_forty_and_honors_offset() {
        let values = (0..100).collect::<Vec<_>>();
        assert_eq!(
            paginate(values.clone(), &PageQuery::default()).len(),
            DEFAULT_PAGE_SIZE
        );

        let page = paginate(
            values,
            &PageQuery {
                limit: Some(3),
                offset: Some(4),
            },
        );
        assert_eq!(page, vec![4, 5, 6]);
    }
}
