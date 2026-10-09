// core/src/enforcer.rs

pub struct SecurityEnforcer {
    profiles: std::collections::HashMap<u64, PetalProfile>,
}

impl SecurityEnforcer {
    pub fn new() -> Self {
        Self { profiles: std::collections::HashMap::new() }
    }

    pub fn register_petal(&mut self, profile: PetalProfile) {
        self.profiles.insert(profile.petal_id, profile);
    }

    pub fn check_access(&self, petal_id: u64, required_right: PetalRights) -> Result<(), &'static str> {
        let profile = self.profiles.get(&petal_id).ok_or("Unknown petal ID")?;
        
        if !profile.rights.contains(required_right) {
            return Err("Permission denied: Missing required capability");
        }
        Ok(())
    }

    pub fn check_sector_access(&self, petal_id: u64, target_sector: u32, is_write: bool) -> Result<(), &'static str> {
        let profile = self.profiles.get(&petal_id).ok_or("Unknown petal ID")?;
        let required_right = if is_write { PetalRights::WRITE_SECTOR } else { PetalRights::READ_SECTOR };
        self.check_access(petal_id, required_right)?;
        let allowed_sectors = unsafe { 
            std::slice::from_raw_parts(profile.allowed_sectors_ptr as *const u32, profile.allowed_sectors_len as usize) 
        };

        if !allowed_sectors.contains(&target_sector) {
            return Err("Permission denied: Sector not in allowed list");
        }

        Ok(())
    }
    pub fn check_ipc_target(&self, sender_id: u64, target_id: u64) -> Result<(), &'static str> {
        self.check_access(sender_id, PetalRights::IPC_SEND)?;
        
        let profile = self.profiles.get(&sender_id).unwrap();
        let allowed_targets = unsafe { 
            std::slice::from_raw_parts(profile.allowed_targets_ptr as *const u64, profile.allowed_targets_len as usize) 
        };

        if !allowed_targets.contains(&target_id) {
            return Err("Permission denied: IPC target not allowed");
        }

        Ok(())
    }
}