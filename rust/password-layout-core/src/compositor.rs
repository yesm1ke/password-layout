//! The window compositor the service works through. Stage 1 has the real
//! Hyprland socket client coming in a later stage; tests and the parity
//! driver use [`FakeCompositor`].

pub type Pid = i32;

pub trait Compositor {
    fn layout_index(&mut self) -> std::io::Result<i32>;
    /// For each configured layout, whether it is Latin. Empty when unknown.
    fn latin_layouts(&mut self) -> std::io::Result<Vec<bool>>;
    fn set_layout(&mut self, index: i32) -> std::io::Result<()>;
    /// 0 when no window has focus.
    fn focused_pid(&mut self) -> std::io::Result<Pid>;
}

/// Public (not cfg(test)): integration tests, the parity driver and later
/// stages use it. Same behaviour as FakeCompositor in tests/test.cpp.
#[derive(Debug, Clone, Default)]
pub struct FakeCompositor {
    pub index: i32,
    pub latin: Vec<bool>,
    pub switches: Vec<i32>,
    pub focused: Pid,
}

impl FakeCompositor {
    pub fn new(index: i32, latin: Vec<bool>) -> Self {
        Self {
            index,
            latin,
            switches: Vec::new(),
            focused: 0,
        }
    }
}

impl Compositor for FakeCompositor {
    fn layout_index(&mut self) -> std::io::Result<i32> {
        Ok(self.index)
    }

    fn latin_layouts(&mut self) -> std::io::Result<Vec<bool>> {
        Ok(self.latin.clone())
    }

    fn set_layout(&mut self, index: i32) -> std::io::Result<()> {
        self.index = index;
        self.switches.push(index);
        Ok(())
    }

    fn focused_pid(&mut self) -> std::io::Result<Pid> {
        Ok(self.focused)
    }
}
