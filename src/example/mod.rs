//! Structs and data classes that are only useful in the serve example.
//! 
//! 
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskBody {
    pub body: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskResponse {
    pub success: bool,
    pub worker: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
