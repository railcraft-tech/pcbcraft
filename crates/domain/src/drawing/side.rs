/// Which face of the board a drawing shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Front of the board, KiCad `F.*`.
    Side,
    /// Bottom of the board, KiCad `B.*`, mirrored.
    Back,
}

impl Side {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "side" => Some(Self::Side),
            "back" => Some(Self::Back),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Side => "side",
            Self::Back => "back",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_two_face_names() {
        assert_eq!(Side::parse("side"), Some(Side::Side));
        assert_eq!(Side::parse("back"), Some(Side::Back));
        assert_eq!(Side::parse("front"), None);
        assert_eq!(Side::Back.as_str(), "back");
    }
}
