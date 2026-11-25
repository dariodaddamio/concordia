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
