#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pixel {
    pub shade: u8,
}

impl Pixel {
    pub fn new(shade: u8) -> Self {
        Self { shade }
    }
}
