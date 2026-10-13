use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CapabilityFlags: u32 {
        const IN_MEMORY = 0x01;
        const TRANSACTIONS = 0x02;
        const SCHEMAS = 0x04;
    }
}
