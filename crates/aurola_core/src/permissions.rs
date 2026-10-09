// core/src/permissions.rs
use bitflags::bitflags;

bitflags! {
    /// Główne prawa (capabilities) Płatka.
    /// Są nadawane raz przy ładowaniu i weryfikowane przy każdej operacji.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PetalRights: u64 {
        // Dostęp do pamięci (sektorów hex)
        const READ_SECTOR     = 1 << 0;
        const WRITE_SECTOR    = 1 << 1;
        const EXEC_CODE       = 1 << 2; // Prawo do wykonywania kodu w swoim sektorze
        
        // Komunikacja (IPC)
        const IPC_SEND        = 1 << 3;
        const IPC_RECV        = 1 << 4;
        
        // Modyfikacje i System
        const SELF_MODIFY     = 1 << 5; // Prawo do żądania aktualizacji samego siebie
        const ALLOC_MEMORY    = 1 << 6; // Prawo do żądania nowego sektora hex od Core
        const NET_ACCESS      = 1 << 7; // Dostęp do sieci (jeśli Aurole potrzebuje)
        const FILE_IO         = 1 << 8; // Dostęp do plików
    }
}

/// Profil bezpieczeństwa Płatka. Zapisywany w binarnym nagłówku samego Płatka 
/// lub przekazywany przez Core przy starcie.
#[repr(C)]
pub struct PetalProfile {
    pub petal_id: u64,
    pub rights: PetalRights,
    // Lista sektorów hex, do których ten Płatek ma dostęp (zgodnie z prawami READ/WRITE)
    pub allowed_sectors_ptr: u64, // Wskaźnik do tablicy u32 (w pamięci Core)
    pub allowed_sectors_len: u32,
    // Lista ID innych Płatków, z którymi może się komunikować (0 = tylko Core)
    pub allowed_targets_ptr: u64, 
    pub allowed_targets_len: u32,
}