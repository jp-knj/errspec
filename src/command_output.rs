//! Owned command output shared by filesystem actions and selected-result commands.
pub(crate) struct Output {
    pub json: serde_json::Value,
    pub text: String,
    pub code: u8,
}
