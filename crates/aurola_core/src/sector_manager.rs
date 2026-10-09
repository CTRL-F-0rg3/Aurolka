use std::collections::HashMap;

#[repr(C)]
pub struct SectorDescriptor {
    pub base_hex_address: u32,
    pub size_bytes: u32,
    pub petal_id: u64,
    pub is_active: bool,
}

pub struct SectorManager {
    sectors: HashMap<u32, SectorDescriptor>,
    petal_sectors: HashMap<u64, Vec<u32>>,
}

impl SectorManager {
    pub fn new() -> Self {
        Self {
            sectors: HashMap::new(),
            petal_sectors: HashMap::new(),
        }
    }

    // Core przypisuje sektor do Płatka podczas jego inicjalizacji
    pub fn assign_sector(&mut self, petal_id: u64, base_hex: u32, size: u32) {
        let desc = SectorDescriptor {
            base_hex_address: base_hex,
            size_bytes: size,
            petal_id,
            is_active: true,
        };
        self.sectors.insert(base_hex, desc);
        self.petal_sectors.entry(petal_id).or_insert_with(Vec::new).push(base_hex);
    }

    pub fn validate_frame(&self, header: &FrameHeader, sender_petal_id: u64) -> Result<(), &'static str> {
        if header.magic != MAGIC_STAR_ROSE {
            return Err("Invalid frame magic number");
        }

        let allowed_sectors = self.petal_sectors.get(&sender_petal_id)
            .ok_or("Unknown sender petal")?;
        
        if !allowed_sectors.contains(&header.src_sector) {
            return Err("Sender spoofing src_sector");
        }
        let dst_desc = self.sectors.get(&header.dst_sector)
            .ok_or("Destination sector does not exist")?;
        
        if !dst_desc.is_active {
            return Err("Destination sector is inactive");
        }
        if header.payload_len > dst_desc.size_bytes {
            return Err("Payload exceeds destination sector size");
        }

        Ok(())
    }
}