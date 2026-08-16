use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct LastProxy {
    pub group: String,
    pub name: String,
}
