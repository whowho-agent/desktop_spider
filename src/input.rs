//! Ввод для плети за один кадр. Собирается в `app.rs` из платформы (хоткей) и событий winit (кнопки).

/// Состояние ввода, общее для платформ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WhipInput {
    /// Хоткей плети удерживается (в режимах Game / Whip).
    pub held: bool,
    /// ЛКМ нажата (уровень, не фронт).
    pub lmb: bool,
    /// ПКМ нажата (уровень, не фронт).
    pub rmb: bool,
}
