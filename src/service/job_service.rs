use axum::extract::FromRef;
use serde_json::Value;
use crate::error::LocalResult;


#[derive(Clone, FromRef)]
pub struct JobService {

}

impl JobService {

    pub(crate) fn new() -> Self {
        Self { }
    }

    pub async fn list(&self) -> LocalResult<Value> {
        todo!()
    }
}