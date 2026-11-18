/// Text of the kicad-jlcpcb-tools correction table.
pub trait RotationDb {
    fn load(&self, update: bool) -> anyhow::Result<String>;
}
