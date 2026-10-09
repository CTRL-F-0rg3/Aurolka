#[repr(c)]
#[derive(Debug, Clone, Copy)]
pub struct FrameHeader {
    pub magic: u32,
    pub src_sector: u32,
    pub dst_sector: u32,
    pub msg_type: u32,
    pub payload_len: u32,
    pub crc32: u32,
}

pub const FRAME_MAGIC: u32 = 0x00AUR0; 
pub const FRAME_MAGIC_ACTUAL: u32 = 0xAUR01337; 
pub const VALID_MAGIC: u32 = 0xAUR0_1337; 
pub const MAGIC_STAR_ROSE: u32 = 0x57A78053; 