//! Shared context message types. This leaf keeps thread rendering independent of context policy.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackRole {
    User,
    Assistant,
    Card,
}

#[derive(Debug, Clone)]
pub struct PackMessage {
    pub role: PackRole,
    pub text: String,
}
